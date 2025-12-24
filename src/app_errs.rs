use crate::AppErr;

impl From<poise::serenity_prelude::Error> for crate::AppErr {
    fn from(value: poise::serenity_prelude::Error) -> Self {
        AppErr::SerenityErr(value)
    }
}
impl From<std::env::VarError> for crate::AppErr {
    fn from(value: std::env::VarError) -> Self {
        AppErr::EnvVarError(value)
    }
}
impl From<std::num::ParseIntError> for crate::AppErr {
    fn from(value: std::num::ParseIntError) -> Self {
        AppErr::ParseIdErr(value)
    }
}
impl From<color_eyre::eyre::Report> for crate::AppErr {
    fn from(value: color_eyre::eyre::Report) -> Self {
        AppErr::AdHocErr(value)
    }
}
impl From<async_sqlite::Error> for crate::AppErr {
    fn from(value: async_sqlite::Error) -> Self {
        AppErr::DatabaseErr(value)
    }
}
