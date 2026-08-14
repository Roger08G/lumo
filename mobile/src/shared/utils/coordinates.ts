export interface Coordinates {
    latitude: number;
    longitude: number;
}

const COORDINATE_NUMBER = /[+-]?\d+(?:[.,]\d+)?/g;

export function parseCoordinates(value: string): Coordinates | null {
    const matches = value.trim().match(COORDINATE_NUMBER);
    if (!matches || matches.length !== 2) return null;

    const latitude = Number(matches[0].replace(",", "."));
    const longitude = Number(matches[1].replace(",", "."));
    if (
        !Number.isFinite(latitude) ||
        !Number.isFinite(longitude) ||
        latitude < -90 ||
        latitude > 90 ||
        longitude < -180 ||
        longitude > 180
    ) {
        return null;
    }

    return { latitude, longitude };
}

export function formatCoordinates({ latitude, longitude }: Coordinates) {
    return `${latitude.toFixed(6)}, ${longitude.toFixed(6)}`;
}
