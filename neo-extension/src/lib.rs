mod big_integer_extensions;
mod byte_extensions;
mod date_time_extensions;
mod random_extensions;
mod string_extensions;
mod utility;

pub mod Collections;
pub mod Net;

pub use assembly_extensions::*;
pub use big_integer_extensions::*;
pub use byte_extensions::*;
pub use date_time_extensions::*;
pub use random_extensions::*;
pub use secure_string_extensions::*;
pub use string_extensions::*;
pub use utility::*;

pub mod log_level;
pub use log_level::*;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
