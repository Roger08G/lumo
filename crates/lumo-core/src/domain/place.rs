use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaceKind {
    Home,
    Shop,
    Medical,
    Place,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaceTone {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaceIcon {
    Home,
    Shopping,
    Health,
    Pin,
    Coffee,
    School,
    Work,
    Park,
    Favorite,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub id: String,
    pub name: String,
    pub address: String,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: u16,
    pub kind: PlaceKind,
    pub color: PlaceTone,
    pub icon: PlaceIcon,
}
