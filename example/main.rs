use bevy::{prelude::*, render::{settings::{Backends, WgpuSettings}, RenderPlugin}};
use bevy_replicon::prelude::*;
use bevy_replicon_lockstep::prelude::*;
use bevy_replicon_renet::RepliconRenetPlugins;
use game_assets::{spawn_unit, Unit, UnitAssets};
use std::{env, time::Duration};
use avian3d::prelude::*;

mod connection;
mod game_assets;

const SIM_TICK_RATE: Duration = Duration::from_millis(33);

/// Command types for the simulation.  Must derive Reflect and be registered
 
/// This is a command it will be broadcast from the server
#[derive(Reflect)]
struct SpawnUnit {
    pub unit_type: Unit,
    pub id: SimulationId,
    pub position: Vec3,
}

/// This is a command to move a unit by applying a fore to its rigidbody
#[derive(Reflect)]
struct ApplyForce {
    force: Vec3,
    target: SimulationId,
}

// Simple component to hold a reference to the actively selected unit
// In this simple example it will just be the last one spawned in
#[derive(Component, Deref, Copy, Clone, Debug)]
struct Selected(SimulationId);

// Store the last Simulation tick we processed
#[derive(Resource, Deref, DerefMut, Default)]
struct LastProcessedTick(u32);

// condition to check if we have a tick ready for processing
fn new_tick_ready(
    cache: Res<LockstepGameCommandBuffer>,
    last_tick: Res<LastProcessedTick>
) -> bool {
    cache.get(last_tick.0 + 1).is_some()
}


fn main() {
    let mut app = App::new();

    // Register our reflected command types
    app.register_type::<SpawnUnit>();
    app.register_type::<ApplyForce>();

    app.add_plugins((
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: Some(Backends::VULKAN),
                    ..default()
                }.into(),
                ..default()
            }),
        PhysicsPlugins::default(),
        PhysicsDebugPlugin::default(),
        // Most lockstep communication is event based. 
        // There are very few replication components and they are not
        // changed often, so every frame is ok
        RepliconPlugins.set(ServerPlugin {
            tick_policy: TickPolicy::EveryFrame,
            ..default()
        }),
        RepliconRenetPlugins,
        RepliconLockstepPlugin {
            simulation: SimulationSettings {
                // ~30 ticks per second
                tick_timestep: SIM_TICK_RATE,
                num_players: 2,
                ..default()
            },
            server: ConnectionSettings {
                server_mode: ServerMode::Host,
                ..default()
            }
        }
    ));
    app.init_resource::<Gravity>();

    // Convenience triggers for connection management
    app.add_observer(connection::start_server.map(|_res| {}));
    app.add_observer(connection::connect_client.map(|_res| {}));
    app.add_observer(connection::stop_server);
    app.add_observer(connection::disconnect_client);
    app.add_observer(connection::on_client_disconnect);
    app.add_observer(connection::on_client_reconnect);

    // parse cli commands to choose host or client mode
    // Run `cargo run server` to start a host server
    if env::args().collect::<Vec<String>>().iter().any(|arg| {arg == "server"}) {
        app.add_systems(Startup, |
            mut commands: Commands,
            mut state: ResMut<NextState<SimulationState>>
            | {
                commands.trigger(connection::TriggerStartServer);
                state.set(SimulationState::Connecting);
            });
    } else { // else it's a client
        app.add_systems(Startup, |
                mut commands: Commands,
                mut state: ResMut<NextState<SimulationState>>
            | {
                commands.trigger(connection::TriggerConnectClient);
                state.set(SimulationState::Connecting);
            });
    }

    // Game related systems
    app
        .init_resource::<LastProcessedTick>()
        .add_systems(
            Startup,
            |mut time: ResMut<Time<Physics>>| {
                // Start with physics paused
                // We have to step it manually for determinism
                // Ticks are not triggered at a regular rate across clients
                time.pause();
            })
        .add_systems(Update, (

            // Initialization logic
            setup_game.run_if(in_state(SimulationState::Setup)),

            // in-game logic
            (   
                // send input commands to server
                send_commands,

                // handle commands from server for all clients
                (
                    update_last_tick,
                    process_tick_commands,
                    step_physics,
                ).run_if(new_tick_ready).chain(),

            ).run_if(in_state(SimulationState::Running))
        ))
        .run();
}

fn setup_game (
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    local_client: Query<Entity, With<LocalClient>>,
    mut loaded: Local<bool>,
    mut ready: Local<bool>,
) {
    // I run this system in update instead of OnEnter(SimulationState::Setup)
    // because LocalClient may not be ready when entering the setup phase
    // You will probably want things like loading progress bars anyway.
    if !*loaded {
        game_assets::setup_environment(&mut commands, &mut meshes, &mut materials);
        *loaded = true;
    }
    if !*ready {
        if let Ok(_) = local_client.get_single() {
            commands.client_trigger(ClientReadyEvent);
            *ready = true;
        }
    }
}


// Handle sending some commands to the server
fn send_commands(
    mut commands: Commands,
    kb: Res<ButtonInput<KeyCode>>,
    sim_tick: Res<SimulationTick>,
    mut count: Local<u16>,
    selected: Query<&Selected>,
) {
    // Create a singleton entity to track which entity is currenty selected
    if selected.get_single().is_err() {
        commands.spawn(Selected(SimulationId::PLACEHOLDER));
    }

    // vec of Commands to be send this tick, if any
    let mut client_commands: Vec<Box<dyn PartialReflect>> = vec![];

    // Spawn a new unit with space bar
    if kb.just_pressed(KeyCode::Space) {
        let x: f32 = (*count % 10) as f32 - 5.0;
        let z: f32 = ((*count - *count % 10) as f32) / 10.0 - 5.0;
        *count += 1;
        let position = Vec3::new(x, 1., z);

        client_commands.push(Box::new(SpawnUnit {
            // Always use PLACEHOLDER when sending SimulationIds to the server
            id: SimulationId::PLACEHOLDER,
            unit_type: Unit::Capsule,
            position,
        }));
    }

    // Move the selected unit (last spawned) around with WASD
    if let Ok(selected) = selected.get_single() {
        let mut force = Vec3::ZERO;
        if kb.pressed(KeyCode::KeyA) {
            force -= Vec3::X;
        }
        if kb.pressed(KeyCode::KeyD) {
            force += Vec3::X;
        }
        if kb.pressed(KeyCode::KeyW) {
            force -= Vec3::Z;
        }
        if kb.pressed(KeyCode::KeyS) {
            force += Vec3::Z;
        }
        if force != Vec3::ZERO {
            force *= 5.0;
            client_commands.push(Box::new(ApplyForce {
                force,
                target: **selected,
            }));
        }
    }

    if client_commands.iter().len() > 0 {
        trace!("SENDING COMMANDS on tick {}", **sim_tick);
        commands.client_trigger(ClientSendCommands {
            commands: client_commands,
            issued_tick: **sim_tick,
        });
    }
}

// Receiving commands from server: 
// This is where you should implement the logic for applying commands for all clients
// The next 3 systems will all be run in the order they are defined below.
// They will be delayed by a few ticks to account for client ping.  This is the downside
// of the lockstep architecture.  The upside is it scales better than other architectures
// when you have massive numbers of entities.

fn update_last_tick(mut last_tick: ResMut<LastProcessedTick>) {
    last_tick.0 += 1;
}

fn process_tick_commands(
    mut commands: Commands,
    command_history: Res<LockstepGameCommandBuffer>,
    assets: Res<UnitAssets>,
    mut selected: Query<&mut Selected>,
    ids: Res<SimulationIdEntityMap>,
    mut forces: Query<&mut ExternalForce>,
    last_tick: Res<LastProcessedTick>,
) {
    let Some(tick_commands) = command_history.get(last_tick.0) else { return };
    for (_client, commands_for_client) in tick_commands.iter() {
        for cmd in commands_for_client.iter() {
            // Is it a SpawnUnit command ?
            if let Some(spawn_cmd) = SpawnUnit::from_reflect(cmd.as_partial_reflect()) {
                // Always use new when spawning a new SimulationId to the server
                // Also make sure the order of spawning is identical for determinism
                let sim_id = SimulationId::new();
                spawn_unit(
                    spawn_cmd.unit_type,
                    Transform::default().with_translation(spawn_cmd.position),
                    sim_id,
                    &mut commands,
                    &assets,
                );
                selected.single_mut().0 = sim_id;
            // Is it an ApplyForce command?
            } else if let Some(force_cmd) = ApplyForce::from_reflect(cmd.as_partial_reflect()) {
                if let Some(unit) = ids.get(&force_cmd.target) {
                    if let Ok(mut unit) = forces.get_mut(*unit) {
                        unit.apply_force(force_cmd.force);
                    }
                }
            }
        }
    }
}

fn step_physics(
    world: &mut World
) {
    world
        .resource_mut::<Time<Physics>>()
        .advance_by(SIM_TICK_RATE);
    world.run_schedule(PhysicsSchedule);
}