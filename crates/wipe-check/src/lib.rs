//! テスト専用。メモリ上の値を「消去してから解放したか」を、解放の直前の中身で確かめる。
//!
//! zeroizeで消去したつもりでも、実際に消えているかは、値が論理的に空になったことからは分からない
//! (`clear()`や`= None`は、中身を上書きせずに、解放する)。かといって、消去した後の領域を読み直すと、解放済みの
//! 領域を読むことになり、未定義動作になる。そこで、アロケータが、解放の直前に、追跡している領域の中身を見る。
//!
//! 使い方: テストの実行ファイルに、`WipeCheckAllocator`をグローバルアロケータとして登録する。
//!
//! ```ignore
//! #[cfg(test)]
//! #[global_allocator]
//! static ALLOCATOR: wipe_check::WipeCheckAllocator = wipe_check::WipeCheckAllocator;
//! ```
//!
//! 追跡したい文字列の実体(ヒープ)を`Watch::track`で登録し、その値を持つ物を捨てた後に、
//! `Watch::assert_all_wiped_when_freed`で、登録した全ての実体が「解放され」「解放の時点で全て0だった」ことを確かめる。
//! 追跡できるのは、文字列の実体の全体で、`String`(の全体を指す`&str`)に限る。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

const SLOT_COUNT: usize = 1024;

const PENDING: u8 = 0;
const FREED_CLEAN: u8 = 1;
const FREED_DIRTY: u8 = 2;

struct Slot {
    reserved: AtomicBool,
    /// 追跡している実体の先頭アドレス。0は、追跡していない(まだ、または、解放を確かめ終えた)ことを表す。
    address: AtomicUsize,
    state: AtomicU8,
}

impl Slot {
    const fn new() -> Self {
        Self { reserved: AtomicBool::new(false), address: AtomicUsize::new(0), state: AtomicU8::new(PENDING) }
    }
}

static SLOTS: [Slot; SLOT_COUNT] = [const { Slot::new() }; SLOT_COUNT];

/// 追跡中の実体の数。0のときは、解放のたびに、枠を調べない(全ての解放を遅くしないため)。
static ARMED: AtomicUsize = AtomicUsize::new(0);

pub struct WipeCheckAllocator;

// 追跡中の実体の解放だけを見る。確保・再確保・0埋めの確保は、標準のアロケータへそのまま渡す。
unsafe impl GlobalAlloc for WipeCheckAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::Acquire) != 0 {
            record_free(ptr, layout.size());
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

// 解放しようとしている実体が追跡中なら、その中身が全て0かを記録する。ここでは、確保しない
// (アロケータの中で確保すると、再帰するため)。
fn record_free(ptr: *mut u8, size: usize) {
    let address = ptr as usize;
    for slot in &SLOTS {
        if slot.address.load(Ordering::Acquire) != address {
            continue;
        }
        if slot.address.compare_exchange(address, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // 解放の直前で、この領域は、まだ確保されている。
            let contents = unsafe { std::slice::from_raw_parts(ptr, size) };
            let state = if contents.iter().all(|byte| *byte == 0) { FREED_CLEAN } else { FREED_DIRTY };
            slot.state.store(state, Ordering::Release);
            ARMED.fetch_sub(1, Ordering::AcqRel);
        }
        return;
    }
}

/// 追跡している実体の集まり。捨てると、追跡を外す。
#[derive(Default)]
pub struct Watch {
    slots: Vec<usize>,
}

impl Watch {
    pub fn new() -> Self {
        Self::default()
    }

    /// 文字列の実体(ヒープ)を追跡する。`text`は、実体の全体(先頭から)を指すこと。空の文字列は、実体が無いため、
    /// 追跡しない。
    pub fn track(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let index = SLOTS
            .iter()
            .position(|slot| slot.reserved.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok())
            .expect("追跡の枠が足りない");
        let slot = &SLOTS[index];
        slot.state.store(PENDING, Ordering::Release);
        slot.address.store(text.as_ptr() as usize, Ordering::Release);
        ARMED.fetch_add(1, Ordering::AcqRel);
        self.slots.push(index);
    }

    pub fn track_opt(&mut self, text: Option<&str>) {
        if let Some(text) = text {
            self.track(text);
        }
    }

    /// 追跡している実体の数。
    pub fn tracked_count(&self) -> usize {
        self.slots.len()
    }

    /// 追跡した全ての実体が、解放され、解放の時点で全て0だったことを確かめる。
    /// 解放されていない実体があるときは、値を捨てた後に呼ぶこと。
    pub fn assert_all_wiped_when_freed(&self) {
        let mut not_freed = 0;
        let mut dirty = 0;
        for &index in &self.slots {
            match SLOTS[index].state.load(Ordering::Acquire) {
                PENDING => not_freed += 1,
                FREED_DIRTY => dirty += 1,
                _ => {}
            }
        }
        assert!(
            not_freed == 0 && dirty == 0,
            "追跡した{}件のうち、まだ解放されていない実体が{}件、消去されないまま解放された実体が{}件ある",
            self.slots.len(),
            not_freed,
            dirty
        );
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for &index in &self.slots {
            let slot = &SLOTS[index];
            // 解放を確かめる前に捨てられた追跡は、外す(枠を、他のテストが使えるようにする)。
            if slot.address.swap(0, Ordering::AcqRel) != 0 {
                ARMED.fetch_sub(1, Ordering::AcqRel);
            }
            slot.reserved.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: WipeCheckAllocator = WipeCheckAllocator;

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn secret_text() -> String {
        // 追跡する実体が、他の確保と取り違えられないよう、一度だけ確保して返す。
        String::from("dummy-secret-value-for-wipe-check")
    }

    // 実体の容量の全体を、0で上書きする(長さは0に戻す。実体は、解放しない)。0は、有効なUTF-8。
    fn wipe_in_place(text: &mut String) {
        let capacity = text.capacity();
        let bytes = unsafe { text.as_mut_vec() };
        bytes.clear();
        bytes.resize(capacity, 0);
        bytes.clear();
    }

    #[test]
    fn a_string_wiped_before_it_is_freed_is_reported_as_wiped() {
        let mut text = secret_text();
        let mut watch = Watch::new();
        watch.track(&text);
        wipe_in_place(&mut text);
        drop(text);

        watch.assert_all_wiped_when_freed();
    }

    #[test]
    fn a_string_freed_without_wiping_is_reported() {
        let text = secret_text();
        let mut watch = Watch::new();
        watch.track(&text);
        drop(text);

        let result = catch_unwind(AssertUnwindSafe(|| watch.assert_all_wiped_when_freed()));
        assert!(result.is_err(), "消去せずに解放したのに、消去済みと判定された");
    }

    #[test]
    fn a_string_that_is_still_alive_is_reported_as_not_freed() {
        let text = secret_text();
        let mut watch = Watch::new();
        watch.track(&text);

        let result = catch_unwind(AssertUnwindSafe(|| watch.assert_all_wiped_when_freed()));
        assert!(result.is_err(), "まだ解放していないのに、確かめが通った");
        drop(text);
    }

    #[test]
    fn empty_strings_have_no_heap_block_and_are_not_tracked() {
        let mut watch = Watch::new();
        watch.track("");
        assert_eq!(watch.tracked_count(), 0);
        watch.assert_all_wiped_when_freed();
    }

    #[test]
    fn dropping_the_watch_releases_its_slots_for_later_use() {
        for _ in 0..(SLOT_COUNT * 2) {
            let text = secret_text();
            let mut watch = Watch::new();
            watch.track(&text);
            // 追跡を外してから、解放する(枠が戻らなければ、枠が尽きる)。
            drop(watch);
            drop(text);
        }
    }
}
