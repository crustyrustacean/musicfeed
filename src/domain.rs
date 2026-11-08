// src/domain.rs

use chrono::NaiveDate;
use serde::Serialize;

#[derive(Clone, Debug, serde::Serialize)]
pub struct RotationEntry {
    pub id: u64,
    pub listened_date: NaiveDate,
    pub artist: String,
    pub album: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewRotationEntry {
    pub listened_date: NaiveDate,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
    pub year: Option<i32>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtistStat {
    pub artist: String,
    pub spins: u64,
}
