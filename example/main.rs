use bevy::{prelude::*, render::{settings::{Backends, WgpuSettings}, RenderPlugin}};
use bevy_replicon::prelude::*;
use bevy_replicon_lockstep::{
    commands::types::MoveCommand,
    prelude::*
};
use bevy_replicon_renet::RepliconRenetPlugins;
use std::{env, time::Duration};
use avian3d::prelude::*;

mod connection;
mod environment;


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
        // Most lockstep communication is message based so we don't need a fast
        // tick rate.  But we do need repliation to happen periodically to 
        // handle client connections.
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
    app.register_type::<MoveCommand>();
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

    app.add_systems(OnEnter(SimulationState::Setup), 
        environment::setup_environment,
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
) {
    if kb.just_pressed(KeyCode::Space) {
        commands.client_trigger(ClientSendCommands {
            commands: vec![
                Box::new(MoveCommand(Vec3::splat(2.0)))
            ],
            issued_tick: **sim_tick,
        });
        info!("SENDING COMMANDS on tick {}", **sim_tick);
    }
}

// Test receiving commands from server on tick
fn receive_commands(
    command_history: Res<LockstepGameCommandBuffer>,
    mut new_ticks: EventReader<SimulationTickUpdate>,
) {
    for new_tick in new_ticks.read() {
        let tick_commands = command_history.get(&new_tick.0).unwrap();
        for (_client, commands) in tick_commands.iter() {
            for cmd in commands.iter() {
                if let Some(move_cmd) = MoveCommand::from_reflect(cmd.as_partial_reflect()) {
                    info!("received move command {}", move_cmd.0);
                }
            }
        }
        // Multiple tick packets may come in at the same time. 
        // Keep this in mind when looping over these tick updates.
        // For example you may need to manually tick the physics sim here.
        break;
    }
}