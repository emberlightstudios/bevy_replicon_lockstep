use bevy::{prelude::*, render::{settings::{Backends, WgpuSettings}, RenderPlugin}};
use bevy_replicon::prelude::*;
use bevy_replicon_lockstep::prelude::*;
use bevy_replicon_renet::RepliconRenetPlugins;
use std::{env, time::Duration};

mod connection;

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
        // Most lockstep communication is message based so we don't need a fast
        // tick rate.  But we do need repliation to happen periodically to 
        // handle client connections.
        RepliconPlugins.set(ServerPlugin {
            tick_policy: TickPolicy::MaxTickRate(15),
            ..default()
        }),
        RepliconRenetPlugins,
        RepliconLockstepPlugin {
            commands: vec![
                String::from("move")
            ],
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
    app.add_observer(connection::start_server.map(|_res| {}));
    app.add_observer(connection::connect_client.map(|_res| {}));
    app.add_observer(connection::stop_server);
    app.add_observer(connection::disconnect_client);

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

    app.add_observer(on_client_disconnect);
    app.add_observer(on_client_reconnect);
    app.add_systems(Update,
        setup_game.run_if(in_state(SimulationState::Setup))
    );
    app.add_systems(Update, test.run_if(in_state(SimulationState::Running)));
    app.run();
}

fn setup_game (
    mut commands: Commands,
    local_client: Query<Entity, (With<LocalClient>, Without<ClientReady>)>,
) {
    // handle loading assets used in the game...
    // when ready inform the server
    if let Ok(_) = local_client.get_single() {
        commands.client_trigger(ClientReadyEvent);
    }
}

fn on_client_disconnect(
    _trigger: Trigger<ClientDisconnect>,
) {
    info!("Client disconnected");
}

fn on_client_reconnect(
    _trigger: Trigger<ClientReconnect>,
) {
    // reconnect logic
    info!("Trying to reconnect to server");
}

fn test(
    mut commands: Commands,
    kb: Res<ButtonInput<KeyCode>>,
    sim_tick: Res<SimulationTick>,
    cmd_types: Res<CommandTypeRegistry>,
) {
    if kb.just_pressed(KeyCode::Space) {
        commands.client_trigger(ClientSendCommands {
            commands: Some(vec![
                LockstepCommand {
                    command_type_id: *cmd_types.get_id("move".to_string()).unwrap(),
                    ..default()
                }
            ]),
            issued_tick: **sim_tick,
        });
    }
}


