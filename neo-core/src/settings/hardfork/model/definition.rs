use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Hardfork {
    Aspidochelone = 0,
    Basilisk = 1,
    Cockatrice = 2,
    Domovoi = 3,
    Echidna = 4,
    Faun = 5,
    Gorgon = 6,
}

impl Hardfork {
    pub const ALL: [Hardfork; 7] = [
        Hardfork::Aspidochelone,
        Hardfork::Basilisk,
        Hardfork::Cockatrice,
        Hardfork::Domovoi,
        Hardfork::Echidna,
        Hardfork::Faun,
        Hardfork::Gorgon,
    ];

    pub fn canonical_name(self) -> &'static str {
        match self {
            Hardfork::Aspidochelone => "HF_Aspidochelone",
            Hardfork::Basilisk => "HF_Basilisk",
            Hardfork::Cockatrice => "HF_Cockatrice",
            Hardfork::Domovoi => "HF_Domovoi",
            Hardfork::Echidna => "HF_Echidna",
            Hardfork::Faun => "HF_Faun",
            Hardfork::Gorgon => "HF_Gorgon",
        }
    }
}

impl fmt::Display for Hardfork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.canonical_name())
    }
}
