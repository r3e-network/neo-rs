pub mod jpath_token;
pub mod jpath_token_type;
pub mod json_error;
pub mod jtoken;
pub mod utility;

pub mod json_convert_trait;

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
