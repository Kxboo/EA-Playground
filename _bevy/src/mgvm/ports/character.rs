//! Character manager accessors.
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
type R = Result<(), String>;

/// CharacterManager::GetCharacter(int) @0x802ec6f8: the slot array starts at the manager itself
pub fn get_character(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, i) = (vm.a(0), vm.a(1));
    let off = i << 2;
    vm.st.cpu.r[0] = off;
    let c = vm.r32(t.wrapping_add(off));
    vm.ret(c);
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[("GetCharacter__16CharacterManagerFi", get_character)];
