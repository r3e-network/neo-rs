use super::super::utils::filter_json;

#[test]
fn filter_matches_csharp_examples() {
    let json = r#"
        {
            "Stores": ["Lambton Quay",  "Willis Street"],
            "Manufacturers": [{
                "Name": "Acme Co",
                "Products": [{ "Name": "Anvil", "Price": 50 }]
            },{
                "Name": "Contoso",
                "Products": [
                    { "Name": "Elbow Grease", "Price": 99.95 },
                    { "Name": "Headlight Fluid", "Price": 4 }
                ]
            }]
        }
        "#;

    assert_eq!(
        r#"["Acme Co"]"#,
        String::from_utf8(filter_json(json, Some("$.Manufacturers[0].Name")).unwrap()).unwrap()
    );
    assert_eq!(
        "[50]",
        String::from_utf8(
            filter_json(json, Some("$.Manufacturers[0].Products[0].Price")).unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        r#"["Elbow Grease"]"#,
        String::from_utf8(
            filter_json(json, Some("$.Manufacturers[1].Products[0].Name")).unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        r#"[{"Name":"Elbow Grease","Price":99.95}]"#,
        String::from_utf8(filter_json(json, Some("$.Manufacturers[1].Products[0]")).unwrap())
            .unwrap()
    );
}
