use super::*;

#[test]
fn test_measurement_to_angle() {
    // Min
    assert_eq!(measurement_to_angle(0), 0);

    // Max
    assert_eq!(measurement_to_angle(27000), 280);
    assert_eq!(measurement_to_angle(64000), 280);

    // Exact
    assert_eq!(measurement_to_angle(26226), 250);
    assert_eq!(measurement_to_angle(26227), 280);
    assert_eq!(measurement_to_angle(19000), 160);
    assert_eq!(measurement_to_angle(19250), 180);

    // Interpolated
    assert_eq!(measurement_to_angle(19126), 175);
}

#[test]
fn test_measurement_to_angle_no_crash() {
    for i in 0..u16::MAX {
        measurement_to_angle(i);
    }
}

#[test]
fn test_map_potentiometer_value() {
    assert_eq!(map_potentiometer_value(0), 100);
    assert_eq!(map_potentiometer_value(26227), 0);
    assert_eq!(map_potentiometer_value(30000), 0);
    assert_eq!(map_potentiometer_value(18700), 50);
}
