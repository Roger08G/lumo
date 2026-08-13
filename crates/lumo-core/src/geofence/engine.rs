use crate::domain::Place;

const EARTH_RADIUS_M: f64 = 6_371_000.0;

pub fn distance_m(first_lat: f64, first_lon: f64, second_lat: f64, second_lon: f64) -> f64 {
    let first_lat = first_lat.to_radians();
    let first_lon = first_lon.to_radians();
    let second_lat = second_lat.to_radians();
    let second_lon = second_lon.to_radians();
    let delta_lat = second_lat - first_lat;
    let delta_lon = second_lon - first_lon;
    let haversine = (delta_lat / 2.0).sin().powi(2)
        + first_lat.cos() * second_lat.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * haversine.sqrt().asin()
}

pub fn containing_place(places: &[Place], latitude: f64, longitude: f64) -> Option<&Place> {
    places
        .iter()
        .filter_map(|place| {
            let distance = distance_m(latitude, longitude, place.latitude, place.longitude);
            (distance <= f64::from(place.radius_m)).then_some((place, distance))
        })
        .min_by(|(_, first), (_, second)| first.total_cmp(second))
        .map(|(place, _)| place)
}

#[cfg(test)]
mod tests {
    use crate::domain::{PlaceIcon, PlaceKind, PlaceTone};

    use super::*;

    #[test]
    fn finds_a_point_inside_a_geofence() {
        let home = Place {
            id: "home".into(),
            name: "Casa".into(),
            address: "Calle".into(),
            latitude: 40.4168,
            longitude: -3.7038,
            radius_m: 50,
            kind: PlaceKind::Home,
            color: PlaceTone::Purple,
            icon: PlaceIcon::Home,
        };
        assert_eq!(
            containing_place(&[home], 40.41681, -3.70381).map(|place| place.id.as_str()),
            Some("home")
        );
    }
}
