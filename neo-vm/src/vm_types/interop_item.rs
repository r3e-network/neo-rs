#[derive(Debug, Clone)]
pub struct InteropItem {
    value: Box<dyn std::any::Any>,
}

impl InteropItem {
    pub fn new(value: Box<dyn std::any::Any>) -> Self {
        Self { value }
    }
}

impl PartialEq for &InteropItem {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(*self, *other) {
            return true;
        }

        // Try to downcast both values to Any + Equatable
        let a = self.value.lock().unwrap();
        let b = other.value.lock().unwrap();

        // Check if both can be cast to Equatable
        let a_eq = a.downcast_ref::<Box<dyn Equatable>>();
        let b_eq = b.downcast_ref::<Box<dyn Equatable>>();

        match (a_eq, b_eq) {
            (Some(a), Some(b)) => a.equals(b),
            (None, None) => std::ptr::eq(a.deref(), b.deref()),
            _ => false
        }
    }
}