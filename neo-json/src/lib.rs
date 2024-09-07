pub mod jarray;
pub mod jtoken;
pub mod jboolean;
pub mod jcontainer;
pub mod ordered_dictionary;
pub mod jnumber;
pub mod ordered_dictionary_key_collection;
pub mod jobject;
pub mod ordered_dictionary_value_collection;
pub mod jpath_token;
pub mod utility;
pub mod jpath_token_type;
pub mod jstring;


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
