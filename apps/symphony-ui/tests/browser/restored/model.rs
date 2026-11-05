//! The models the gallery cases serve, made here rather than read from a project.

/// The smallest model a viewer loads: one triangle, as a binary glTF.
pub(super) fn triangle_glb() -> Vec<u8> {
    let mut json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36,"target":34962}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]}"#.to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let corners: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let length = |bytes: usize| u32::try_from(bytes).unwrap().to_le_bytes();
    let chunk = |kind: &[u8; 4], data: &[u8]| [&length(data.len())[..], kind, data].concat();
    let body = [chunk(b"JSON", &json), chunk(b"BIN\0", &corners)].concat();
    [
        &b"glTF"[..],
        &2u32.to_le_bytes(),
        &length(12 + body.len()),
        &body,
    ]
    .concat()
}
