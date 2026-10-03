//! Attrib (the game's attribute database, VLT) node and collection accessors: tiny leaf functions called millions of
//! times per scenario.
use crate::gekko::Vm;
use crate::mgvm::MgHost;

type V = Vm<MgHost>;
type R = Result<(), String>;

/// Attrib::Node::GetKey() const @0x802d4398: the 64-bit key (r3:r4) when flag 0x80 at +0xf is set, else 0
pub fn node_get_key(_h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let flags = vm.st.mem.r8(t + 0xf) as u32;
    if flags & 0x80 != 0 {
        let (hi, lo) = (vm.r32(t), vm.r32(t + 4));
        vm.st.cpu.r[3] = hi;
        vm.st.cpu.r[4] = lo;
    } else {
        vm.st.cpu.r[4] = 0;
        vm.st.cpu.r[3] = 0;
    }
    vm.st.cpu.r[0] = flags & 0x80;
    Ok(())
}

/// Attrib::Node::GetPointer(void* layout, const Collection*) const @0x802d4f2c
pub fn node_get_pointer(_h: &mut MgHost, vm: &mut V) -> R {
    let (t, layout, coll) = (vm.a(0), vm.a(1), vm.a(2));
    let flags = vm.st.mem.r8(t + 0xf) as u32;
    vm.st.cpu.r[6] = flags;
    let r = if flags & 0x40 != 0 {
        t + 8
    } else if flags & 0x10 != 0 {
        layout.wrapping_add(vm.r32(t + 8))
    } else if flags & 0x20 != 0 {
        let c = vm.r32(coll + 0x18);
        vm.st.cpu.r[4] = c;
        let cls = vm.r32(c + 8);
        vm.r32(cls + 0x30).wrapping_add(vm.r32(t + 8))
    } else {
        vm.r32(t + 8)
    };
    vm.ret(r);
    Ok(())
}

/// Attrib::Attribute::GetElementPointer(unsigned int) const @0x802f49e8: the single element when +0xc is set, else
/// tail call GetInternalPointer
pub fn attribute_get_element_pointer(h: &mut MgHost, vm: &mut V) -> R {
    let (t, i) = (vm.a(0), vm.a(1));
    let p = vm.r32(t + 0xc);
    vm.st.cpu.r[0] = p;
    if p != 0 {
        vm.ret(if i == 0 { p } else { 0 });
        return Ok(());
    }
    vm.call_by_name(h, "GetInternalPointer__Q26Attrib9AttributeCFUi", &[t, i], &[])?;
    Ok(())
}

/// Attrib::Class::GetCollection(unsigned long long) const @0x802d3534: tail call the collection table's Find
pub fn class_get_collection(h: &mut MgHost, vm: &mut V) -> R {
    let t = vm.a(0);
    let table = vm.r32(t + 8).wrapping_add(0x18);
    let (r4, r5, r6) = (vm.a(1), vm.a(2), vm.a(3));
    vm.call_by_name(h, "Find__70VecHashMap<Ux,Q26Attrib10Collection,Q36Attrib5Class11TablePolicy,1,96>CFUx", &[table, r4, r5, r6], &[])?;
    Ok(())
}

/// pgDBCollection::GetInternalIterator() @0x802f4944 (returned by value: r3 is the result, r4 this)
pub fn pgdb_get_internal_iterator(_h: &mut MgHost, vm: &mut V) -> R {
    let (out, t) = (vm.a(0), vm.a(1));
    let v = vm.r32(t + 8);
    vm.st.cpu.r[0] = v;
    vm.w32(out, v);
    Ok(())
}

pub const PORTS: &[crate::mgvm::ports::Port] = &[
    ("GetKey__Q26Attrib4NodeCFv", node_get_key),
    ("GetPointer__Q26Attrib4NodeCFPvPCQ26Attrib10Collection", node_get_pointer),
    ("GetElementPointer__Q26Attrib9AttributeCFUi", attribute_get_element_pointer),
    ("GetCollection__Q26Attrib5ClassCFUx", class_get_collection),
    ("GetInternalIterator__14pgDBCollectionFv", pgdb_get_internal_iterator),
];
