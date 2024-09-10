// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

struct Defer<F: FnOnce()> {
    action: Option<F>,
}

/// impl Drop for Defer
impl<F: FnOnce()> Drop for Defer<F> {
    fn drop(&mut self) {
        self.action.take().map(|action| action());
    }
}

/// defer a function call
pub fn defer<F: FnOnce()>(action: F) -> impl Drop {
    Defer {
        action: Some(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defer() {
        let mut x = 0;
        {
            let _d = defer(|| x += 1);
        }
        assert_eq!(x, 1);
    }
}
