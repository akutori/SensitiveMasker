//! 副作用のない純粋なマスキングロジック(Functional Core)。ファイルI/O・DB・GUIを一切知らない。

pub mod masker;
pub mod matcher;
pub mod models;

pub use masker::{apply_compiled_profile, apply_profile, CompiledProfile, MappingStore, RuleMatchCount};
pub use matcher::{find_matches, MatchSpan};
pub use models::{
    validate_display_name, Mode, PatternType, Rule, RuleError, RuleProfile, RuleProfileError,
    MAX_DISPLAY_NAME_LENGTH,
};
