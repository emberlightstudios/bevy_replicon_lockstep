use bevy::{prelude::*, render::{settings::{Backends, WgpuSettings}, RenderPlugin}};
use bevy_replicon::prelude::*;
use bevy_replicon_lockstep::prelude::*;
use bevy_replicon_renet::RepliconRenetPlugins;
use game_assets::{spawn_unit, Unit, UnitAssets};
use std::{env, time::Duration};
use avian3d::prelude::*;

mod connection;
mod game_assets;

/// This is a command it will be broadcast from the server
#[derive(Reflect)]
pub struct SpawnUnit{
    pub unit_type: Unit,
    pub id: SimulationId,
    pub position: Vec3,
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: Some(Backends::VULKAN),
                    ..default()
                }.into(),
                ..default()
            }),
        // I put this before the lockstep plugin because it inserts its own 
        // Fixed<Time> resource
        PhysicsPlugins::default(),
        // Most lockstep communication is event based so we don't need a fast tick rate.  
        RepliconPlugins.set(ServerPlugin {
            tick_policy: TickPolicy::EveryFrame,
            ..default()
        }),
        RepliconRenetPlugins,
        RepliconLockstepPlugin {
            simulation: SimulationSettings {
                // ~30 ticks per second
                tick_timestep: Duration::from_millis(33),
                num_players: 2,
                ..default()
            },
            server: ConnectionSettings {
                server_mode: ServerMode::Host,
                ..default()
            }
        }
    ));
    app.register_type::<SpawnUnit>();
    app.register_type::<Unit>();
    app.add_observer(connection::start_server.map(|_res| {}));
    app.add_observer(connection::connect_client.map(|_res| {}));
    app.add_observer(connection::stop_server);
    app.add_observer(connection::disconnect_client);
    app.add_observer(connection::on_client_disconnect);
    app.add_observer(connection::on_client_reconnect);

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

    app.add_systems(OnEnter(SimulationState::Ending), cleanup);
    app.add_systems(OnEnter(SimulationState::Setup), 
        game_assets::setup_environment,
    );
    app.add_systems(Update, 
        setup_game.run_if(in_state(SimulationState::Setup))
    );
    app.add_systems(Update, (
        send_command,
        receive_commands,
    )
        .run_if(in_state(SimulationState::Running))
    );
    app.run();
}

fn setup_game (
    mut commands: Commands,
    local_client: Query<Entity, (With<LocalClient>, Without<ClientReady>)>,
) {
    // This part needs to run in a loop because the setup phase may have been entered
    // before the local client_id was received.  Other logic can run here also,
    // e.g. loading progress bars.
    if let Ok(_) = local_client.get_single() {
        commands.client_trigger(ClientReadyEvent);
    }
}

// Testing sending commands to server
fn send_command(
    mut commands: Commands,
    kb: Res<ButtonInput<KeyCode>>,
    sim_tick: Res<SimulationTick>,
    mut count: Local<u16>,
) {
    if kb.just_pressed(KeyCode::Space) {
        let x: f32 = (*count % 10) as f32 - 5.0;
        let z: f32 = ((*count - *count % 10) as f32) / 10.0 - 5.0;
        let position = Vec3::new(x, 0., z);
        commands.client_trigger(ClientSendCommands {
            commands: vec![
                Box::new(SpawnUnit {
                    // Always use PLACEHOLDER when sending to the server
                    id: SimulationId::PLACEHOLDER,
                    unit_type: Unit::Capsule,
                    position,
                })
            ],
            issued_tick: **sim_tick,
        });
        *count += 1;
        info!("SENDING COMMANDS on tick {}", **sim_tick);
    }
}

// Test receiving commands from server on tick
fn receive_commands(
    mut commands: Commands,
    command_history: Res<LockstepGameCommandBuffer>,
    mut new_ticks: EventReader<SimulationTickUpdate>,
    assets: Res<UnitAssets>,
) {
    for new_tick in new_ticks.read() {
        let Some(tick_commands) = command_history.get(new_tick.0) else { return };
        for (_client, commands_for_client) in tick_commands.iter() {
            for cmd in commands_for_client.iter() {
                if let Some(spawn_cmd) = SpawnUnit::from_reflect(cmd.as_partial_reflect()) {
                    // Always use new when spawning a new component to the server
                    let sim_id = SimulationId::new();
                    info!("{}", spawn_cmd.position);
                    spawn_unit(
                        spawn_cmd.unit_type,
                        Transform::default().with_translation(spawn_cmd.position),
                        sim_id,
                        &mut commands,
                        &assets,
                    );
                }
            }
        }
        // Multiple tick packets may come in at the same time. 
        // Keep this in mind when looping over these tick updates.
        // For example you may need to manually tick the physics sim here.
        break;
    }
}

fn cleanup(mut commands: Commands) {
    commands.remove_resource::<UnitAssets>();
}