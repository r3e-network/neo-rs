use std::cmp::Ordering;

pub struct ByteArrayComparer {
    direction: i32,
}

impl ByteArrayComparer {
    pub const DEFAULT: Self = ByteArrayComparer { direction: 1 };
    pub const REVERSE: Self = ByteArrayComparer { direction: -1 };

    fn new(direction: i32) -> Self {
        ByteArrayComparer { direction }
    }

    pub fn compare(&self, x: Option<&[u8]>, y: Option<&[u8]>) -> i32 {
        match (x, y) {
            (Some(x), Some(y)) if x.as_ptr() == y.as_ptr() => 0,
            (None, Some(y)) => if self.direction > 0 { -(y.len() as i32) } else { y.len() as i32 },
            (Some(x), None) => if self.direction > 0 { x.len() as i32 } else { -(x.len() as i32) },
            (Some(x), Some(y)) => {
                if self.direction > 0 {
                    Self::compare_internal(x, y)
                } else {
                    -Self::compare_internal(x, y)
                }
            },
            (None, None) => 0,
        }
    }

    #[inline(always)]
    fn compare_internal(x: &[u8], y: &[u8]) -> i32 {
        let length = x.len().min(y.len());
        for i in 0..length {
            match x[i].cmp(&y[i]) {
                Ordering::Equal => continue,
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
            }
        }
        x.len().cmp(&y.len()) as i32
    }
}

impl Ord for ByteArrayComparer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.direction.cmp(&other.direction)
    }
}

impl PartialOrd for ByteArrayComparer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ByteArrayComparer {
    fn eq(&self, other: &Self) -> bool {
        self.direction == other.direction
    }
}

impl Eq for ByteArrayComparer {}
