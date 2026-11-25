use super::*;

const IDENTITY_2: &str = "TITLE \"identity\"\n# comment\nLUT_3D_SIZE 2\n\
0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

#[test]
fn a_cube_file_parses_in_red_fastest_order() {
    let lut = parse_cube(IDENTITY_2).unwrap();
    assert_eq!(lut.size, 2);
    assert_eq!(lut.data[1], [1.0, 0.0, 0.0]);
    assert_eq!(lut.data[2], [0.0, 1.0, 0.0]);
    assert_eq!(lut.data[7], [1.0, 1.0, 1.0]);
}

#[test]
fn the_domain_is_normalized_to_zero_one() {
    let text =
        "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n".to_string() + &"2 2 2\n".repeat(8);
    let lut = parse_cube(&text).unwrap();
    assert_eq!(lut.data[0], [1.0, 1.0, 1.0]);
}

#[test]
fn broken_cubes_are_errors() {
    assert!(parse_cube("LUT_3D_SIZE 2\n0 0 0\n").is_err());
    assert!(parse_cube("0 0 0\n").is_err());
    assert!(parse_cube("LUT_1D_SIZE 4\n").is_err());
    assert!(parse_cube("LUT_3D_SIZE 2\n0 0 x\n").is_err());
}

#[test]
fn a_registered_path_loads_once() {
    let dir = std::env::temp_dir().join(format!("vv-lut-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("id.cube");
    std::fs::write(&path, IDENTITY_2).unwrap();
    let id = lut_id(&path);
    assert_eq!(id, lut_id(&path));
    assert_eq!(lut_for_id(id).unwrap().unwrap().size, 2);
    assert!(validate(&path).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}
