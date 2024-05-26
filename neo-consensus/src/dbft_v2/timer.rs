// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::dbft_v2::ViewNumber;


#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub struct HView {
    /// The height of the chain, i.e. block number
    pub height: u32,

    /// The view number in DBFT v2.0
    pub view_number: ViewNumber,
}

impl HView {
    #[inline]
    pub fn zero(&self) -> bool { self.eq(&Self::default()) }
}

pub struct Timer {
    //
}