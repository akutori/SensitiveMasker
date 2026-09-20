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
//!
//! 登録できない実体(ライブラリの内部で作られる複製・伸長で捨てられる旧バッファ)は、`MarkerScan`で数える。テストの値に、
//! そのテスト専用の目印の文字列を含めておき、`MarkerScan::start`から`finish`までの間に、目印を含んだまま解放された実体の
//! 数を返す(0なら、消去されない複製が、残っていない)。
//!
//! `WipeCheckAllocator`は、確保した領域を、全て0で埋める(解放の直前に読む領域に、未初期化のバイトが無く、前の確保の残りが、
//! 別の確保の中の目印として数えられないようにするため。テスト専用のため、遅くなってよい)。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

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

// 枠の番号を、u16のビットで表すため、16を超えない。
const SCAN_SLOT_COUNT: usize = 16;

struct ScanSlot {
    reserved: AtomicBool,
    /// 探す文字列(`start`が、解放せずに残した箱に入れた、staticな文字列)を指す。nullは、使っていないことを表す。
    /// 先頭アドレスと長さを、1回の読み取りで、組として読ませるため、箱を指す(別々の値にすると、枠が、次の数え上げへ
    /// 再利用される瞬間に、前の長さと次のアドレスを組み合わせて読むことがある)。
    marker: AtomicPtr<&'static str>,
    /// 解放の時点で、中身に目印を含んでいた実体の数。
    unwiped_frees: AtomicUsize,
    /// 直近に数えた実体の大きさ(診断用)。0は、まだ数えていないこと。
    last_size: AtomicUsize,
    /// 直近に数えた実体が、再確保で移されたものか(診断用)。
    last_via_realloc: AtomicBool,
}

impl ScanSlot {
    const fn new() -> Self {
        Self {
            reserved: AtomicBool::new(false),
            marker: AtomicPtr::new(std::ptr::null_mut()),
            unwiped_frees: AtomicUsize::new(0),
            last_size: AtomicUsize::new(0),
            last_via_realloc: AtomicBool::new(false),
        }
    }
}

static SCAN_SLOTS: [ScanSlot; SCAN_SLOT_COUNT] = [const { ScanSlot::new() }; SCAN_SLOT_COUNT];

/// 目印の数え上げが動いている数。0のときは、解放のたびに、中身を見ない。
static SCANS_ARMED: AtomicUsize = AtomicUsize::new(0);

pub struct WipeCheckAllocator;

// 解放(と、再確保)の直前の中身を見るため、確保した領域は、全て0で埋める。標準のアロケータが返す領域は、未初期化で、
// 前の確保の残り(消去せずに解放された、目印の断片)を含みうる。それを読むと、未初期化の領域を読むことになり、
// 前の持ち主の複製を、今の持ち主の複製として数えてしまう。
unsafe impl GlobalAlloc for WipeCheckAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { realloc_with(ptr, layout, new_size, |ptr, layout, new_size| System.realloc(ptr, layout, new_size)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::Acquire) != 0 {
            record_free(ptr, layout.size());
        }
        if SCANS_ARMED.load(Ordering::Acquire) != 0 {
            scan_free(ptr, layout.size());
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

// 再確保の数え方。標準の再確保を、差し替えられる形にしてある(テストが、必ず移す・移さない再確保を渡して、数え方を、
// アロケータの挙動に頼らずに確かめるため)。
unsafe fn realloc_with(
    ptr: *mut u8,
    layout: Layout,
    new_size: usize,
    system_realloc: impl FnOnce(*mut u8, Layout, usize) -> *mut u8,
) -> *mut u8 {
    // 別の場所へ移されると、旧い実体は、消去されずに解放される。移される前に、目印を含むかを調べておく。
    let matched = if SCANS_ARMED.load(Ordering::Acquire) != 0 { scan_matches(ptr, layout.size()) } else { 0 };
    let new_ptr = system_realloc(ptr, layout, new_size);
    if new_ptr.is_null() {
        return new_ptr;
    }
    // 縮める再確保は、数えない: 縮めるときに、別の場所へ移すかは、アロケータ次第で(Windowsは、移すことがある)、ライブラリの
    // 内部(ageが、復号した1チャンクのVecを、into_boxed_sliceで縮める)にも、あるため、アプリの複製の確認に、揺らぎが混ざる。
    if new_size > layout.size() {
        // 伸びた分は、未初期化のため、0で埋める(確保した領域を、全て0で埋める、という約束を、保つ)。
        unsafe { std::ptr::write_bytes(new_ptr.add(layout.size()), 0, new_size - layout.size()) };
        // その場で伸びただけなら、中身は、まだ使われているため、数えない。
        if matched != 0 && new_ptr != ptr {
            count_matches(matched, layout.size(), true);
        }
    }
    new_ptr
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

// 解放しようとしている実体の中身に、数え上げ中の目印が含まれていれば、その数え上げの件数を増やす。ここでは、確保しない
// (アロケータの中で確保すると、再帰するため)。
fn scan_free(ptr: *mut u8, size: usize) {
    count_matches(scan_matches(ptr, size), size, false);
}

// 実体の中身に含まれる目印を持つ数え上げの枠を、ビットの集まり(枠の番号のビット)で返す。
fn scan_matches(ptr: *mut u8, size: usize) -> u16 {
    let mut matched = 0u16;
    for (index, slot) in SCAN_SLOTS.iter().enumerate() {
        let marker = slot.marker.load(Ordering::Acquire);
        if marker.is_null() {
            continue;
        }
        // 箱は、解放せずに残してあるため、枠が、別の数え上げへ再利用された後でも、有効なままである。
        let marker = unsafe { *marker }.as_bytes();
        if marker.len() > size {
            continue;
        }
        // 解放(再確保)の直前で、この領域は、まだ確保されており、確保のときに、全て0で埋めてある(初期化済み)。
        let contents = unsafe { std::slice::from_raw_parts(ptr, size) };
        if contents.windows(marker.len()).any(|window| window == marker) {
            matched |= 1 << index;
        }
    }
    matched
}

fn count_matches(matched: u16, size: usize, via_realloc: bool) {
    for (index, slot) in SCAN_SLOTS.iter().enumerate() {
        if matched & (1 << index) != 0 {
            slot.last_size.store(size, Ordering::Relaxed);
            slot.last_via_realloc.store(via_realloc, Ordering::Relaxed);
            slot.unwiped_frees.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// 目印の文字列を含んだまま解放された実体を数える(`start`から`finish`までの間)。
///
/// 目印は、そのテスト専用の文字列にする(並行して走る他のテストの値と、重ならないように)。数えるのは、目印を含む実体の
/// 解放の時点の中身だけで、消去してから解放した実体は、数えない。テストの入力(目印を含む値)は、`start`の前に作り、
/// `finish`の後に捨てる(入力の解放を、数えないため)。
pub struct MarkerScan {
    slot: usize,
}

impl MarkerScan {
    pub fn start(marker: &'static str) -> Self {
        assert!(!marker.is_empty(), "目印が空");
        let slot = SCAN_SLOTS
            .iter()
            .position(|slot| slot.reserved.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok())
            .expect("数え上げの枠が足りない");
        let entry = &SCAN_SLOTS[slot];
        entry.unwiped_frees.store(0, Ordering::Release);
        // 箱は、解放せずに残す(解放の側が、枠の再利用と同時に、前の箱を読んでも、無効にならないように)。数え上げ1回につき、
        // 1つの&strの大きさだけ。
        let marker: &'static mut &'static str = Box::leak(Box::new(marker));
        entry.marker.store(marker, Ordering::Release);
        SCANS_ARMED.fetch_add(1, Ordering::AcqRel);
        Self { slot }
    }

    /// 数え上げを終え、目印を含んだまま解放された実体の数を返す。
    pub fn finish(self) -> usize {
        SCAN_SLOTS[self.slot].unwiped_frees.load(Ordering::Acquire)
        // dropが、枠を戻す。
    }

    /// 直近に数えた実体の(大きさ、再確保で移されたものか)。診断用: 数えた実体が、何によるものかを、失敗の文言へ添える。
    pub fn last_hit(&self) -> (usize, bool) {
        let slot = &SCAN_SLOTS[self.slot];
        (slot.last_size.load(Ordering::Relaxed), slot.last_via_realloc.load(Ordering::Relaxed))
    }
}

impl Drop for MarkerScan {
    fn drop(&mut self) {
        let entry = &SCAN_SLOTS[self.slot];
        entry.marker.store(std::ptr::null_mut(), Ordering::Release);
        SCANS_ARMED.fetch_sub(1, Ordering::AcqRel);
        entry.reserved.store(false, Ordering::Release);
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

    // 縮める再確保は、数えない(移る・移らないが、アロケータ次第で揺らぐため)。
    #[test]
    fn a_block_shrunk_by_a_reallocation_is_not_counted() {
        const MARKER: &str = "marker-scan-test-shrink-3d81f4";
        let scan = MarkerScan::start(MARKER);

        for _ in 0..200 {
            let mut bytes = Vec::with_capacity(4096);
            bytes.extend_from_slice(MARKER.as_bytes());
            bytes.shrink_to_fit();
            bytes.iter_mut().for_each(|byte| *byte = 0);
        }

        assert_eq!(scan.finish(), 0, "縮めただけの再確保が、数えられた");
    }

    // 目印は、テストごとに、別の文字列にする(並行して走る、他のテストの値と重ならないように)。
    #[test]
    fn a_block_freed_with_the_marker_inside_is_counted() {
        const MARKER: &str = "marker-scan-test-counted-4f2a91";
        let copy = String::from(MARKER);
        let scan = MarkerScan::start(MARKER);

        drop(copy);

        assert!(scan.finish() >= 1, "目印を含んだまま解放したのに、数えられなかった");
    }

    // Vecの伸長で、別の場所へ移されると、旧い実体は、消去されずに解放される。大きく伸ばして、移させる(その場で伸びた場合は、
    // 中身が、まだ使われているため、数えない。移るかは、アロケータ次第のため、繰り返して、1件以上を確かめる)。
    #[test]
    fn a_block_moved_by_a_reallocation_with_the_marker_inside_is_counted() {
        const MARKER: &str = "marker-scan-test-realloc-5e90ab";
        let scan = MarkerScan::start(MARKER);

        for _ in 0..200 {
            let mut bytes = Vec::with_capacity(64);
            bytes.extend_from_slice(MARKER.as_bytes());
            bytes.reserve(256 * 1024);
            // 移った先の実体は、消去してから解放する(数えられるのは、移された旧い実体だけになる)。
            bytes.iter_mut().for_each(|byte| *byte = 0);
        }

        assert!(scan.finish() >= 1, "移された旧い実体が、1件も数えられなかった");
    }

    // ---- 再確保の数え方: 標準の再確保を、必ず移す・移さないものへ差し替えて、アロケータの挙動に頼らずに確かめる ----

    // 必ず別の場所へ移す再確保(新しい領域を確保して、複製し、旧い領域を解放する。標準のアロケータを直接呼ぶため、フックを通らない)。
    fn always_moving_realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let new_layout = Layout::from_size_align(new_size, layout.align()).unwrap();
            let new_ptr = System.alloc_zeroed(new_layout);
            std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
            System.dealloc(ptr, layout);
            new_ptr
        }
    }

    // その場で伸びた再確保を模す(領域は、新しい大きさ以上で、確保してある)。
    fn never_moving_realloc(ptr: *mut u8, _layout: Layout, _new_size: usize) -> *mut u8 {
        ptr
    }

    // 目印を先頭に持つ領域を、標準のアロケータで確保する(この確保・解放は、フックを通らない)。
    fn block_with_marker(marker: &str, size: usize) -> (*mut u8, Layout) {
        let layout = Layout::from_size_align(size, 1).unwrap();
        let ptr = unsafe { System.alloc_zeroed(layout) };
        assert!(!ptr.is_null());
        unsafe { std::ptr::copy_nonoverlapping(marker.as_ptr(), ptr, marker.len()) };
        (ptr, layout)
    }

    fn layout_of(size: usize) -> Layout {
        Layout::from_size_align(size, 1).unwrap()
    }

    #[test]
    fn a_growth_that_moves_the_block_counts_the_old_block() {
        const MARKER: &str = "marker-scan-test-grow-moved-1a2b3c";
        let (ptr, layout) = block_with_marker(MARKER, 256);
        let scan = MarkerScan::start(MARKER);

        let new_ptr = unsafe { realloc_with(ptr, layout, 4096, always_moving_realloc) };

        assert_eq!(scan.finish(), 1, "移された旧い実体が、数えられなかった");
        unsafe { System.dealloc(new_ptr, layout_of(4096)) };
    }

    #[test]
    fn a_shrink_that_moves_the_block_is_not_counted() {
        const MARKER: &str = "marker-scan-test-shrink-moved-4d5e6f";
        let (ptr, layout) = block_with_marker(MARKER, 4096);
        let scan = MarkerScan::start(MARKER);

        let new_ptr = unsafe { realloc_with(ptr, layout, 128, always_moving_realloc) };

        assert_eq!(scan.finish(), 0, "縮めるだけの再確保が、数えられた");
        unsafe { System.dealloc(new_ptr, layout_of(128)) };
    }

    #[test]
    fn a_growth_in_place_is_not_counted() {
        const MARKER: &str = "marker-scan-test-grow-inplace-7a8b9c";
        let (ptr, _) = block_with_marker(MARKER, 4096);
        let scan = MarkerScan::start(MARKER);

        // 256バイトとして確保された領域が、その場で、4096バイトへ伸びた(中身は、まだ使われている)。
        let new_ptr = unsafe { realloc_with(ptr, layout_of(256), 4096, never_moving_realloc) };

        assert_eq!(new_ptr, ptr);
        assert_eq!(scan.finish(), 0, "その場で伸びただけの再確保が、数えられた");
        unsafe { System.dealloc(new_ptr, layout_of(4096)) };
    }

    #[test]
    fn the_part_added_by_a_growth_is_zero_filled() {
        // 再確保が、伸びた部分に、前の残り(0以外)を持つ領域を返しても、伸びた部分は、0で埋まる。
        let dirty = unsafe { System.alloc(layout_of(1024)) };
        assert!(!dirty.is_null());
        unsafe { std::ptr::write_bytes(dirty, 0xAA, 1024) };

        let new_ptr = unsafe { realloc_with(dirty, layout_of(64), 1024, never_moving_realloc) };

        let grown = unsafe { std::slice::from_raw_parts(new_ptr.add(64), 1024 - 64) };
        assert!(grown.iter().all(|byte| *byte == 0), "伸びた部分が、0で埋まっていない");
        unsafe { System.dealloc(new_ptr, layout_of(1024)) };
    }

    // 確保した領域は、全て0で埋まっている: 前の確保が、目印を含んだまま(消去せずに)解放されていても、その残りが、次の確保の、
    // 書かれていない部分に、現れない(現れると、前の持ち主の複製が、今の持ち主の複製として、数えられる)。
    #[test]
    fn a_new_block_does_not_inherit_the_marker_left_in_a_freed_block() {
        const MARKER: &str = "marker-scan-test-stale-9f8e7d";
        // 数え上げの外で、目印を含んだまま解放する(この解放は、数えない)。目印は、次に書く先頭の数バイトより後ろに置く。
        let mut stale = Vec::<u8>::with_capacity(4096);
        stale.resize(512, b'.');
        stale.extend_from_slice(MARKER.as_bytes());
        drop(stale);

        let scan = MarkerScan::start(MARKER);
        // 同じ大きさの確保は、直前に解放された領域が、再利用されやすい。先頭の数バイトだけを書く。
        let mut fresh = Vec::<u8>::with_capacity(4096);
        fresh.extend_from_slice(b"short");
        drop(fresh);

        assert_eq!(scan.finish(), 0, "前の確保の残りが、別の確保の中の目印として、数えられた");
    }

    #[test]
    fn a_block_wiped_before_it_is_freed_is_not_counted() {
        const MARKER: &str = "marker-scan-test-wiped-7c31d0";
        let mut copy = String::from(MARKER);
        let scan = MarkerScan::start(MARKER);

        wipe_in_place(&mut copy);
        drop(copy);

        assert_eq!(scan.finish(), 0, "消去してから解放したのに、数えられた");
    }

    #[test]
    fn a_block_freed_before_the_scan_started_is_not_counted() {
        const MARKER: &str = "marker-scan-test-before-91be55";
        let copy = String::from(MARKER);
        drop(copy);

        let scan = MarkerScan::start(MARKER);

        assert_eq!(scan.finish(), 0);
    }

    #[test]
    fn blocks_without_the_marker_are_not_counted() {
        const MARKER: &str = "marker-scan-test-absent-2ad8c7";
        let scan = MarkerScan::start(MARKER);

        drop(String::from("something else entirely, without the marker"));
        drop(vec![7u8; 4096]);

        assert_eq!(scan.finish(), 0);
    }

    #[test]
    fn finishing_releases_the_slot_for_later_scans() {
        for _ in 0..(SCAN_SLOT_COUNT * 2) {
            MarkerScan::start("marker-scan-test-slot-c0de12").finish();
        }
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
