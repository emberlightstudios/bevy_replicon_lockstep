use bevy::{math::VectorSpace, prelude::*, utils::hashbrown::HashMap};
use bevy_replicon_lockstep::prelude::SimulationId;
use avian3d::prelude::*;

#[derive(Eq, PartialEq, Hash, Reflect)]
pub enum Unit {
    Capsule,
}

pub fn spawn_unit(
    unit: Unit,
    transform: Transform,
    id: SimulationId,
    commands: &mut Commands,
    assets: &Res<UnitAssets>,
) -> Entity {
    match unit {
        Unit::Capsule => {
            return commands.spawn((
                Mesh3d(assets.meshes.get(&Unit::Capsule).unwrap().clone()),
                MeshMaterial3d(assets.materials.get(&Unit::Capsule).unwrap().clone()),
                transform.clone(),
                id,
                RigidBody::Dynamic,
                Collider::capsule(0.5, 1.),
                LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                Friction::new(0.1),
                ExternalForce::new(Vec3::ZERO).with_persistence(false),
            )).id();
        }
    }
}

#[derive(Resource)]
pub struct UnitAssets {
    pub meshes: HashMap<Unit, Handle<Mesh>>,
    pub materials: HashMap<Unit, Handle<StandardMaterial>>,
}

pub fn setup_environment(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let floor_material = materials.add(Color::linear_rgb(0.3, 0.7, 0.3));
    let floor_mesh = meshes.add(Plane3d::default().mesh().size(50.0, 50.0));

    let unit_mesh = meshes.add(Capsule3d::default());
    let unit_material = materials.add(Color::linear_rgb(0.5, 0.5, 0.5));
    let mut unit_meshes = HashMap::<Unit, Handle<Mesh>>::new();
    let mut unit_materials = HashMap::<Unit, Handle<StandardMaterial>>::new();
    unit_meshes.insert(Unit::Capsule, unit_mesh.clone());
    unit_materials.insert(Unit::Capsule, unit_material.clone());
    commands.insert_resource(UnitAssets{
        meshes: unit_meshes,
        materials: unit_materials,
    });

    // Floor
    commands.spawn((
        Mesh3d(floor_mesh),
        MeshMaterial3d(floor_material),
        PickingBehavior::IGNORE,
        RigidBody::Static,
        Collider::cuboid(50., 0.01, 50.0),
        Friction::new(0.1),
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