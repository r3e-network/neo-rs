use crate::smartcontract::binding::Override;
use crate::smartcontract::binding::NewOverrideFromString;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml;

    #[test]
    fn test_new_override_from_string() {
        let test_cases = vec![
            (Override { package: "import.com/pkg".to_string(), type_name: "pkg.Type".to_string() }, "import.com/pkg.Type"),
            (Override { package: "".to_string(), type_name: "map[int]int".to_string() }, "map[int]int"),
            (Override { package: "".to_string(), type_name: "[]int".to_string() }, "[]int"),
            (Override { package: "".to_string(), type_name: "map[int][]int".to_string() }, "map[int][]int"),
            (Override { package: "import.com/pkg".to_string(), type_name: "map[int]pkg.Type".to_string() }, "map[int]import.com/pkg.Type"),
            (Override { package: "import.com/pkg".to_string(), type_name: "[]pkg.Type".to_string() }, "[]import.com/pkg.Type"),
            (Override { package: "import.com/pkg".to_string(), type_name: "map[int]*pkg.Type".to_string() }, "map[int]*import.com/pkg.Type"),
            (Override { package: "import.com/pkg".to_string(), type_name: "[]*pkg.Type".to_string() }, "[]*import.com/pkg.Type"),
            (Override { package: "import.com/pkg".to_string(), type_name: "[][]*pkg.Type".to_string() }, "[][]*import.com/pkg.Type"),
            (Override { package: "import.com/pkg".to_string(), type_name: "map[string][]pkg.Type".to_string() }, "map[string][]import.com/pkg.Type"),
        ];

        for (expected, value) in test_cases {
            assert_eq!(expected, NewOverrideFromString(value.to_string()));

            let s = serde_yaml::to_string(&expected).unwrap();
            assert_eq!(value, s.trim());
        }
    }
}
