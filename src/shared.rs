#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadingState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Error(String),
}

pub const ONE_SECOND_MS: i64 = 1000;
pub const ONE_MINUTE_MS: i64 = ONE_SECOND_MS * 60;
pub const ONE_HOUR_MS: i64 = ONE_MINUTE_MS * 60;
pub const ONE_DAY_MS: i64 = ONE_HOUR_MS * 24;
