use std::error::Error;
use neo_core2::smartcontract::{self, manifest};
use neo_core2::smartcontract::manifest::standard::{Standard, Comply, ComplyABI, Check, CheckABI};
use neo_core2::smartcontract::manifest::standard::errors::{ErrMethodMissing, ErrInvalidReturnType, ErrInvalidParameterCount, ErrInvalidParameterType, ErrInvalidParameterName, ErrEventMissing, ErrSafeMethodMismatch};

fn foo_method_bar_event() -> manifest::Manifest {
    manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "foo".to_string(),
                    parameters: vec![
                        manifest::Parameter { r#type: smartcontract::ParameterType::ByteArray },
                        manifest::Parameter { r#type: smartcontract::ParameterType::PublicKey },
                    ],
                    return_type: smartcontract::ParameterType::Integer,
                    safe: true,
                },
            ],
            events: vec![
                manifest::Event {
                    name: "bar".to_string(),
                    parameters: vec![
                        manifest::Parameter { r#type: smartcontract::ParameterType::String },
                    ],
                },
            ],
        },
    }
}

#[test]
fn test_comply_missing_method() {
    let mut m = foo_method_bar_event();
    m.abi.get_method_mut("foo", -1).unwrap().name = "notafoo".to_string();
    let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
    assert!(err.downcast_ref::<ErrMethodMissing>().is_some());
}

#[test]
fn test_comply_invalid_return_type() {
    let mut m = foo_method_bar_event();
    m.abi.get_method_mut("foo", -1).unwrap().return_type = smartcontract::ParameterType::Void;
    let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
    assert!(err.downcast_ref::<ErrInvalidReturnType>().is_some());
}

#[test]
fn test_comply_method_parameter_count() {
    // Method
    {
        let mut m = foo_method_bar_event();
        let f = m.abi.get_method_mut("foo", -1).unwrap();
        f.parameters.push(manifest::Parameter { r#type: smartcontract::ParameterType::Boolean });
        let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
        assert!(err.downcast_ref::<ErrMethodMissing>().is_some());
    }
    // Event
    {
        let mut m = foo_method_bar_event();
        let ev = m.abi.get_event_mut("bar").unwrap();
        ev.parameters.clear();
        let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
        assert!(err.downcast_ref::<ErrInvalidParameterCount>().is_some());
    }
}

#[test]
fn test_comply_parameter_type() {
    // Method
    {
        let mut m = foo_method_bar_event();
        m.abi.get_method_mut("foo", -1).unwrap().parameters[0].r#type = smartcontract::ParameterType::InteropInterface;
        let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
        assert!(err.downcast_ref::<ErrInvalidParameterType>().is_some());
    }
    // Event
    {
        let mut m = foo_method_bar_event();
        m.abi.get_event_mut("bar").unwrap().parameters[0].r#type = smartcontract::ParameterType::InteropInterface;
        let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
        assert!(err.downcast_ref::<ErrInvalidParameterType>().is_some());
    }
}

#[test]
fn test_comply_parameter_name() {
    // Method
    {
        let mut m = foo_method_bar_event();
        m.abi.get_method_mut("foo", -1).unwrap().parameters[0].name = Some("hehe".to_string());
        let s = Standard { manifest: foo_method_bar_event() };
        let err = Comply(&m, &s).unwrap_err();
        assert!(err.downcast_ref::<ErrInvalidParameterName>().is_some());
        assert!(ComplyABI(&m, &s).is_ok());
    }
    // Event
    {
        let mut m = foo_method_bar_event();
        m.abi.get_event_mut("bar").unwrap().parameters[0].name = Some("hehe".to_string());
        let s = Standard { manifest: foo_method_bar_event() };
        let err = Comply(&m, &s).unwrap_err();
        assert!(err.downcast_ref::<ErrInvalidParameterName>().is_some());
        assert!(ComplyABI(&m, &s).is_ok());
    }
}

#[test]
fn test_missing_event() {
    let mut m = foo_method_bar_event();
    m.abi.get_event_mut("bar").unwrap().name = "notabar".to_string();
    let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
    assert!(err.downcast_ref::<ErrEventMissing>().is_some());
}

#[test]
fn test_safe_flag() {
    let mut m = foo_method_bar_event();
    m.abi.get_method_mut("foo", -1).unwrap().safe = false;
    let err = Comply(&m, &Standard { manifest: foo_method_bar_event() }).unwrap_err();
    assert!(err.downcast_ref::<ErrSafeMethodMismatch>().is_some());
}

#[test]
fn test_comply_valid() {
    let mut m = foo_method_bar_event();
    m.abi.methods.push(manifest::Method {
        name: "newmethod".to_string(),
        offset: 123,
        return_type: smartcontract::ParameterType::ByteArray,
        ..Default::default()
    });
    m.abi.events.push(manifest::Event {
        name: "otherevent".to_string(),
        parameters: vec![manifest::Parameter {
            name: Some("names do not matter".to_string()),
            r#type: smartcontract::ParameterType::Integer,
        }],
    });
    assert!(Comply(&m, &Standard { manifest: foo_method_bar_event() }).is_ok());
}

#[test]
fn test_check() {
    let mut m = manifest::Manifest::new("Test");
    assert!(Check(&m, manifest::NEP17_STANDARD_NAME).is_err());

    m.abi.methods.extend(DecimalTokenBase.abi.methods.clone());
    m.abi.methods.extend(Nep17.abi.methods.clone());
    m.abi.events.extend(Nep17.abi.events.clone());
    assert!(Check(&m, manifest::NEP17_STANDARD_NAME).is_ok());
    assert!(CheckABI(&m, manifest::NEP17_STANDARD_NAME).is_ok());
}

#[test]
fn test_optional() {
    let mut m = Standard::default();
    m.optional = vec![manifest::Method {
        name: "optMet".to_string(),
        parameters: vec![manifest::Parameter { r#type: smartcontract::ParameterType::ByteArray }],
        return_type: smartcontract::ParameterType::Integer,
        ..Default::default()
    }];

    // wrong parameter count, do not check
    {
        let mut actual = manifest::Manifest::default();
        actual.abi.methods = vec![manifest::Method {
            name: "optMet".to_string(),
            return_type: smartcontract::ParameterType::Integer,
            ..Default::default()
        }];
        assert!(Comply(&actual, &m).is_ok());
    }
    // good parameter count, bad return
    {
        let mut actual = manifest::Manifest::default();
        actual.abi.methods = vec![manifest::Method {
            name: "optMet".to_string(),
            parameters: vec![manifest::Parameter { r#type: smartcontract::ParameterType::Array }],
            return_type: smartcontract::ParameterType::Integer,
            ..Default::default()
        }];
        assert!(Comply(&actual, &m).is_err());
    }
    // good parameter count, good return
    {
        let mut actual = manifest::Manifest::default();
        actual.abi.methods = vec![manifest::Method {
            name: "optMet".to_string(),
            parameters: vec![manifest::Parameter { r#type: smartcontract::ParameterType::ByteArray }],
            return_type: smartcontract::ParameterType::Integer,
            ..Default::default()
        }];
        assert!(Comply(&actual, &m).is_ok());
    }
}
