use std::cmp::{max, min};
use std::collections::BTreeMap;
use std::io;
use std::ops::Bound::Included;

/// Represents the initial size of cache.
const INITIAL_CACHE_SIZE: usize = 65536;
/// Represents the max size of cache.
const MAX_CACHE_SIZE: usize = 16 * INITIAL_CACHE_SIZE;

/// Represents the max distance of u32 values between packets in an u32 window.
const MAX_U32_WINDOW_SIZE: usize = 4194304;

/// Represents the linear cache.
#[derive(Debug)]
pub struct Cacher {
    buffer: Vec<u8>,
    sequence: u32,
    head: usize,
    size: usize,
}

impl Cacher {
    /// Creates a new `Cacher`.
    pub fn new(sequence: u32) -> Cacher {
        Cacher {
            buffer: vec![0; INITIAL_CACHE_SIZE],
            sequence,
            head: 0,
            size: 0,
        }
    }

    /// Appends some bytes to the end of the cache.
    pub fn append(&mut self, buffer: &[u8]) -> io::Result<()> {
        if buffer.len() > self.buffer.len() - self.size {
            // Extend the buffer
            let size = min(
                max(self.buffer.len() * 2, self.buffer.len() + buffer.len()),
                MAX_CACHE_SIZE,
            );
            if self.size + buffer.len() > size {
                return Err(io::Error::new(io::ErrorKind::Other, "cache is full"));
            }

            let mut new_buffer = vec![0u8; size];

            // From the head to the end of the buffer
            let length_a = min(self.size, self.buffer.len() - self.head);
            new_buffer[..length_a].copy_from_slice(&self.buffer[self.head..self.head + length_a]);

            // From the begin of the buffer to the tail
            let length_b = self.size - length_a;
            if length_b > 0 {
                new_buffer[length_a..length_a + length_b].copy_from_slice(&self.buffer[..length_b]);
            }

            self.buffer = new_buffer;
            self.head = 0;
        }

        // From the tail to the end of the buffer
        let mut length_a = 0;
        if self.head + self.size < self.buffer.len() {
            length_a = min(buffer.len(), self.buffer.len() - (self.head + self.size));
            self.buffer[self.head + self.size..self.head + self.size + length_a]
                .copy_from_slice(&buffer[..length_a]);
        }

        // From the begin of the buffer to the head
        let length_b = buffer.len() - length_a;
        if length_b > 0 {
            self.buffer[..length_b].copy_from_slice(&buffer[length_a..]);
        }

        self.size += buffer.len();

        Ok(())
    }

    // Invalidates cache to the certain sequence.
    pub fn invalidate_to(&mut self, sequence: u32) {
        let size = sequence
            .checked_sub(self.sequence)
            .unwrap_or_else(|| u32::MAX - self.sequence + sequence) as usize;

        if size <= MAX_U32_WINDOW_SIZE as usize {
            self.sequence = sequence;
            self.size = self.size.checked_sub(size).unwrap_or(0);
            if self.size == 0 {
                self.head = 0;
            } else {
                self.head = (self.head + (size % self.buffer.len())) % self.buffer.len();
            }
        }
    }

    /// Get the buffer from the beginning of the cache in the given size.
    pub fn get(&self, size: usize) -> io::Result<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }
        if self.size < size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "request size too big",
            ));
        }

        let mut vector = vec![0u8; size];

        // From the head to the end of the buffer
        let length_a = min(size, self.buffer.len() - self.head);
        vector[..length_a].copy_from_slice(&self.buffer[self.head..self.head + length_a]);

        // From the begin of the buffer to the tail
        let length_b = size - length_a;
        if length_b > 0 {
            vector[length_a..].copy_from_slice(&self.buffer[..length_b]);
        }

        Ok(vector)
    }

    /// Get all the buffer of the cache.
    pub fn get_all(&self) -> io::Result<Vec<u8>> {
        self.get(self.get_size())
    }

    /// Get the sequence of the cache.
    pub fn get_sequence(&self) -> u32 {
        self.sequence
    }

    /// Get the size of the cache.
    pub fn get_size(&self) -> usize {
        self.size
    }
}

/// Represents the random cache.
#[derive(Debug)]
pub struct RandomCacher {
    buffer: Vec<u8>,
    sequence: u32,
    head: usize,
    /// Represents the expected size from the head to the tail. NOT all the bytes in [head, head + size) are existed.
    size: usize,
    /// Represents ranges of existing values. Use an u64 instead of an u32 because the sequence is used as a ring.
    ranges: BTreeMap<u64, usize>,
}

impl RandomCacher {
    /// Creates a new `RandomCacher`.
    pub fn new(sequence: u32) -> RandomCacher {
        RandomCacher {
            buffer: vec![0u8; INITIAL_CACHE_SIZE],
            sequence,
            head: 0,
            size: 0,
            ranges: BTreeMap::new(),
        }
    }

    /// Appends some bytes to the cache and returns continuous bytes from the beginning.
    pub fn append(&mut self, sequence: u32, buffer: &[u8]) -> io::Result<Option<Vec<u8>>> {
        let sub_sequence = sequence
            .checked_sub(self.sequence)
            .unwrap_or_else(|| sequence + (u32::MAX - self.sequence))
            as usize;
        if sub_sequence > MAX_U32_WINDOW_SIZE {
            return Ok(None);
        }

        let size = sub_sequence + buffer.len();
        if size > self.buffer.len() {
            // Extend the buffer
            let size = min(max(self.buffer.len() * 2, size), MAX_CACHE_SIZE);
            if self.buffer.len() + buffer.len() > size {
                return Err(io::Error::new(io::ErrorKind::Other, "cache is full"));
            }

            let mut new_buffer = vec![0u8; size];

            // TODO: the procedure may by optimized to copy valid bytes only
            // From the head to the end of the buffer
            new_buffer[..self.buffer.len() - self.head].copy_from_slice(&self.buffer[self.head..]);

            // From the begin of the buffer to the tail
            if self.head > 0 {
                new_buffer[self.buffer.len() - self.head..self.buffer.len()]
                    .copy_from_slice(&self.buffer[..self.head]);
            }

            self.buffer = new_buffer;
            self.head = 0;
        }

        // TODO: the procedure may by optimized to copy valid bytes only
        // To the end of the buffer
        let mut length_a = 0;
        if self.buffer.len() - self.head > sub_sequence {
            length_a = min(self.buffer.len() - self.head - sub_sequence, buffer.len());
            self.buffer[self.head + sub_sequence..self.head + sub_sequence + length_a]
                .copy_from_slice(&buffer[..length_a]);
        }

        // From the begin of the buffer
        let length_b = buffer.len() - length_a;
        if length_b > 0 {
            self.buffer[..length_b].copy_from_slice(&buffer[length_a..]);
        }

        // Update size
        let tail = sequence
            .checked_add(buffer.len() as u32)
            .unwrap_or_else(|| buffer.len() as u32 - (u32::MAX - sequence));
        let record_tail = self
            .sequence
            .checked_add(self.size as u32)
            .unwrap_or_else(|| self.size as u32 - (u32::MAX - self.sequence));
        let sub_tail = tail
            .checked_sub(record_tail)
            .unwrap_or_else(|| tail + (u32::MAX - record_tail));
        if sub_tail as usize <= MAX_U32_WINDOW_SIZE {
            self.size += sub_tail as usize;
        }

        // Insert and merge ranges
        {
            let mut sequence = sequence as u64;
            if (sequence as u32) < self.sequence {
                sequence += u32::MAX as u64;
            }

            // Select ranges which can be merged
            let mut pop_keys = Vec::new();
            let mut end = sequence + buffer.len() as u64;
            for (&key, &value) in self.ranges.range((
                Included(&sequence),
                Included(&(sequence + buffer.len() as u64)),
            )) {
                pop_keys.push(key);
                end = max(end, key + value as u64);
            }

            // Pop
            for ref pop_key in pop_keys {
                self.ranges.remove(pop_key);
            }

            // Select the previous range if exists
            let mut prev_key = None;
            for &key in self.ranges.keys() {
                if key < sequence {
                    prev_key = Some(key);
                }
            }

            // Merge previous range
            let mut size = buffer.len();
            if let Some(prev_key) = prev_key {
                let prev_size = *self.ranges.get(&prev_key).unwrap();
                if prev_key + (prev_size as u64) >= sequence {
                    size += (sequence - prev_key) as usize;
                    sequence = prev_key;
                }
            }

            // Insert range
            self.ranges.insert(sequence, size);
        }

        // Pop if possible
        let first_key = *self.ranges.keys().next().unwrap();
        if first_key as u32 == self.sequence {
            let size = self.ranges.remove(&first_key).unwrap();

            // Shrink range sequence is possible
            if ((u32::MAX - self.sequence) as usize) < size {
                let keys: Vec<_> = self.ranges.keys().map(|x| *x).collect();

                for key in keys {
                    let value = self.ranges.remove(&key).unwrap();
                    self.ranges.insert(key - u32::MAX as u64, value);
                }
            }

            let mut vector = vec![0u8; size];

            // From the head to the end of the buffer
            let length_a = min(size, self.buffer.len() - self.head);
            vector[..length_a].copy_from_slice(&self.buffer[self.head..self.head + length_a]);

            // From the begin of the buffer to the tail
            let length_b = size - length_a;
            if length_b > 0 {
                vector[length_a..].copy_from_slice(&self.buffer[..length_b]);
            }

            self.sequence = self
                .sequence
                .checked_add(size as u32)
                .unwrap_or_else(|| size as u32 - (u32::MAX - self.sequence));
            self.head = (self.head + (size % self.buffer.len())) % self.buffer.len();
            self.size -= vector.len();

            return Ok(Some(vector));
        }

        Ok(None)
    }

    /// Get the sequence of the cache.
    pub fn get_sequence(&self) -> u32 {
        self.sequence
    }

    /// Get the remaining size of the `RandomCacher`.
    pub fn get_remaining_size(&self) -> u16 {
        if self.buffer.len() - self.size > u16::MAX as usize {
            u16::MAX
        } else {
            (self.buffer.len() - self.size) as u16
        }
    }
}
