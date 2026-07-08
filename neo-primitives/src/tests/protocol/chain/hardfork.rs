use super::*;

#[test]
fn test_hardfork_all() {
    let all = Hardfork::all();
    assert_eq!(all.len(), 8);
    assert_eq!(Hardfork::COUNT, 8);
    assert_eq!(Hardfork::ALL, all);
    assert_eq!(all[0], Hardfork::HfAspidochelone);
    assert_eq!(all[6], Hardfork::HfGorgon);
    assert_eq!(all[7], Hardfork::HfHuyao);
}

#[test]
fn test_hardfork_index() {
    assert_eq!(Hardfork::HfAspidochelone.index(), 0);
    assert_eq!(Hardfork::HfBasilisk.index(), 1);
    assert_eq!(Hardfork::HfCockatrice.index(), 2);
    assert_eq!(Hardfork::HfDomovoi.index(), 3);
    assert_eq!(Hardfork::HfEchidna.index(), 4);
    assert_eq!(Hardfork::HfFaun.index(), 5);
    assert_eq!(Hardfork::HfGorgon.index(), 6);
    assert_eq!(Hardfork::HfHuyao.index(), 7);
}

#[test]
fn test_hardfork_from_index() {
    assert_eq!(Hardfork::from_index(0), Some(Hardfork::HfAspidochelone));
    assert_eq!(Hardfork::from_index(6), Some(Hardfork::HfGorgon));
    assert_eq!(Hardfork::from_index(7), Some(Hardfork::HfHuyao));
    assert_eq!(Hardfork::from_index(8), None);
    assert_eq!(Hardfork::from_index(255), None);
}

#[test]
fn test_hardfork_from_str() {
    assert_eq!(
        "HF_ASPIDOCHELONE".parse::<Hardfork>().unwrap(),
        Hardfork::HfAspidochelone
    );
    assert_eq!(
        "aspidochelone".parse::<Hardfork>().unwrap(),
        Hardfork::HfAspidochelone
    );
    assert_eq!(
        "ASP".parse::<Hardfork>().unwrap(),
        Hardfork::HfAspidochelone
    );
    assert_eq!(
        "HF_BASILISK".parse::<Hardfork>().unwrap(),
        Hardfork::HfBasilisk
    );
    assert_eq!(
        "basilisk".parse::<Hardfork>().unwrap(),
        Hardfork::HfBasilisk
    );
    assert_eq!("HF_Huyao".parse::<Hardfork>().unwrap(), Hardfork::HfHuyao);
    assert_eq!("huyao".parse::<Hardfork>().unwrap(), Hardfork::HfHuyao);
}

#[test]
fn test_hardfork_from_str_invalid() {
    assert!("unknown".parse::<Hardfork>().is_err());
    assert!("".parse::<Hardfork>().is_err());
}

#[test]
fn test_hardfork_display() {
    assert_eq!(Hardfork::HfAspidochelone.to_string(), "HF_Aspidochelone");
    assert_eq!(Hardfork::HfBasilisk.to_string(), "HF_Basilisk");
    assert_eq!(Hardfork::HfGorgon.to_string(), "HF_Gorgon");
    assert_eq!(Hardfork::HfHuyao.to_string(), "HF_Huyao");
}

#[test]
fn test_hardfork_name() {
    assert_eq!(Hardfork::HfAspidochelone.name(), "HF_Aspidochelone");
    assert_eq!(Hardfork::HfEchidna.name(), "HF_Echidna");
}

#[test]
fn test_hardfork_ordering() {
    assert!(Hardfork::HfAspidochelone < Hardfork::HfBasilisk);
    assert!(Hardfork::HfBasilisk < Hardfork::HfCockatrice);
    assert!(Hardfork::HfFaun < Hardfork::HfGorgon);
    assert!(Hardfork::HfGorgon < Hardfork::HfHuyao);
}

#[test]
fn test_hardfork_try_from_u8() {
    assert_eq!(Hardfork::try_from(0u8).unwrap(), Hardfork::HfAspidochelone);
    assert_eq!(Hardfork::try_from(6u8).unwrap(), Hardfork::HfGorgon);
    assert_eq!(Hardfork::try_from(7u8).unwrap(), Hardfork::HfHuyao);
    assert!(Hardfork::try_from(8u8).is_err());
}

#[test]
fn test_hardfork_serde() {
    let hf = Hardfork::HfEchidna;
    let json = serde_json::to_string(&hf).unwrap();
    assert_eq!(json, "\"HfEchidna\"");
    let parsed: Hardfork = serde_json::from_str(&json).unwrap();
    assert_eq!(hf, parsed);
}
