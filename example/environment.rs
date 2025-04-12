use bevy::prelude::*;

pub fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let floor_material = materials.add(Color::linear_rgb(0.3, 0.7, 0.3));
    let unit_material = materials.add(Color::linear_rgb(0.5, 0.5, 0.5));
    let floor_mesh = meshes.add(Plane3d::default().mesh().size(50.0, 50.0));
    let unit_mesh = meshes.add(Capsule3d::default());

    // Units
    for i in 0..10 {
        for j in 0..10 {
            let x = 2.0 * i as f32 - 10.0;
            let z = 2.0 * j as f32 - 10.0;
            commands.spawn((
                Mesh3d(unit_mesh.clone()),
                MeshMaterial3d(unit_material.clone()),
                Transform::from_translation(Vec3::new(x, 1.0, z)),
            ))
            ;
        }
    }

    // Floor
    commands.spawn((
        Mesh3d(floor_mesh),
        MeshMaterial3d(floor_material),
        PickingBehavior::IGNORE,
    ));

    // Light
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 30., 14.0).looking_at(Vec3::new(0., 1., 0.), Vec3::Y),
    ));


}