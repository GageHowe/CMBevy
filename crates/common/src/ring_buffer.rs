use std::collections::VecDeque;

// note: using VedDeque is actually efficient, no conversion to array needed
// vecdeque is implemented as a circular buffer internally, no allocation happens after initialization

/// A simple fixed-capacity ring buffer
pub struct RingBuffer<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}

impl<T> Default for RingBuffer<T> {
    fn default() -> Self {
        Self::new(128)
    }
}

impl<T> RingBuffer<T> {
    /// Create a new ring buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        RingBuffer { buffer: VecDeque::with_capacity(capacity), capacity }
    }

    /// Push an item into the ring buffer
    /// If at capacity, removes the oldest item
    pub fn push(&mut self, item: T) {
        if self.buffer.len() == self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(item);
    }

    /// Get the number of items currently in the buffer
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Check if the buffer is at capacity
    pub fn is_full(&self) -> bool {
        self.buffer.len() == self.capacity
    }

    /// Get item by position where 0 is the most recent.
    /// Out-of-range positions clamp to the newest item.
    /// Returns None only when the buffer is empty.
    pub fn get(&self, position: usize) -> Option<&T> {
        if self.buffer.is_empty() {
            return None;
        }

        // Clamp out-of-range to newest
        let pos = if position >= self.buffer.len() { 0 } else { position };

        // VecDeque front is oldest, so newest is at (len - 1 - pos)
        self.buffer.get(self.buffer.len() - 1 - pos)
    }

    /// Get a reference to the newest (most recently pushed) item
    #[must_use]
    pub fn get_newest(&self) -> Option<&T> {
        self.buffer.back()
    }

    /// Get a reference to the oldest item
    #[must_use]
    pub fn get_oldest(&self) -> Option<&T> {
        self.buffer.front()
    }

    /// Get an iterator over the items (oldest to newest)
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buffer.iter()
    }

    /// Remove and return the oldest item, or None if empty
    pub fn pop(&mut self) -> Option<T> {
        self.buffer.pop_front()
    }

    /// Clear all items from the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_push() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);

        let items: Vec<_> = rb.iter().copied().collect();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn test_overflow() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // Should remove 1

        let items: Vec<_> = rb.iter().copied().collect();
        assert_eq!(items, vec![2, 3, 4]);
    }

    #[test]
    fn test_with_strings() {
        let mut rb = RingBuffer::new(2);
        rb.push("hello".to_string());
        rb.push("world".to_string());
        rb.push("foo".to_string());

        let items: Vec<_> = rb.iter().map(|s| s.as_str()).collect();
        assert_eq!(items, vec!["world", "foo"]);
    }

    #[test]
    fn test_pop() {
        let mut rb = RingBuffer::new(3);
        assert_eq!(rb.pop(), None);

        rb.push(1);
        rb.push(2);
        rb.push(3);

        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        rb.push(4);

        let items: Vec<_> = rb.iter().copied().collect();
        assert_eq!(items, vec![3, 4]);
    }

    #[test]
    fn test_is_full() {
        let mut rb = RingBuffer::new(2);
        assert!(!rb.is_full());
        rb.push(1);
        assert!(!rb.is_full());
        rb.push(2);
        assert!(rb.is_full());
        rb.push(3); // overwrites, still full
        assert!(rb.is_full());
    }

    #[test]
    fn test_get_newest_oldest() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        assert_eq!(rb.get_newest(), None);
        assert_eq!(rb.get_oldest(), None);

        rb.push(10);
        rb.push(20);
        rb.push(30);

        assert_eq!(rb.get_newest(), Some(&30));
        assert_eq!(rb.get_oldest(), Some(&10));

        rb.push(40); // drops 10
        assert_eq!(rb.get_newest(), Some(&40));
        assert_eq!(rb.get_oldest(), Some(&20));
    }

    #[test]
    fn test_get_by_position() {
        let mut rb = RingBuffer::new(4);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4);

        // 0 = newest, 3 = oldest
        assert_eq!(rb.get(0), Some(&4));
        assert_eq!(rb.get(1), Some(&3));
        assert_eq!(rb.get(2), Some(&2));
        assert_eq!(rb.get(3), Some(&1));
    }

    #[test]
    fn test_get_clamps_out_of_range() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);

        // Out of range clamps to newest
        assert_eq!(rb.get(99), Some(&3));
    }

    #[test]
    fn test_get_on_empty() {
        let rb: RingBuffer<i32> = RingBuffer::new(3);
        assert_eq!(rb.get(0), None);
    }
}
