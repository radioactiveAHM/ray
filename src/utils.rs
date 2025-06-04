#[inline(always)]
pub fn convert_two_u8s_to_u16_be(bytes: [u8; 2]) -> u16 {
    ((bytes[0] as u16) << 8) | bytes[1] as u16
}

#[inline(always)]
pub fn convert_u16_to_two_u8s_be(integer: u16) -> [u8; 2] {
    [(integer >> 8) as u8, integer as u8]
}

#[inline(always)]
pub fn unsafe_staticref<'a, T: ?Sized>(r: &'a T) -> &'static T {
    unsafe { std::mem::transmute::<&'a T, &'static T>(r) }
}

#[allow(mutable_transmutes)]
#[inline(always)]
#[allow(clippy::mut_from_ref)]
pub fn unsafe_refmut<'a, T: ?Sized>(r: &'a T) -> &'a mut T {
    unsafe { std::mem::transmute::<&'a T, &mut T>(r) }
}

#[inline(always)]
pub fn catch_in_buff(find: &[u8], buff: &[u8]) -> Option<(usize, usize)> {
    if find.len() >= buff.len() {
        return None;
    }
    buff.windows(find.len())
        .position(|pre| pre == find)
        .map(|a| (a, a + find.len()))
}
