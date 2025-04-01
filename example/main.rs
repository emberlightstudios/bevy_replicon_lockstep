use bevy::{prelude::*, render::{settings::{Backends, WgpuSettings}, RenderPlugin}};
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use bevy_replicon_lockstep::prelude::*;
use bevy_replicon_renet::RepliconRenetPlugins;
use std::{env, time::Duration};
use dotenv::dotenv;

mod connection;

fn main() {
    dotenv().ok();
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
        RepliconPlugins.set(ServerPlugin {
            tick_policy: TickPolicy::Manual,
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
            }
        }
    ));
    app.add_observer(connection::start_server.map(|_res| {}));
    app.add_observer(connection::connect_client.map(|_res| {}));
    app.add_observer(connection::stop_server);
    app.add_observer(connection::disconnect_client);

    // Run `cargo run server` to start a host server
    if env::args().collect::<Vec<String>>().iter().any(|arg| {arg == "server"}) {
        app.add_systems(Startup, |mut commands: Commands|{
            commands.trigger(connection::TriggerStartServer)
        });
    } else { // else it's a client
        app.add_systems(Startup, |mut commands: Commands| {
            commands.trigger(connection::TriggerConnectClient)
        });
    }

    app.add_systems(Update,
        setup_game.run_if(in_state(SimulationState::Setup))
    );
    app.add_systems(Update, test);
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

fn test(
    mut commands: Commands,
    kb: Res<ButtonInput<KeyCode>>,
    sim_tick: Query<&SimulationTick>,
    _ids: Query<&NetworkId, With<Replicated>>,
    cmd_types: Res<CommandTypeRegistry>,
) {
    let Ok(tick) = sim_tick.get_single() else { return };
    if kb.just_pressed(KeyCode::Space) {
        commands.client_trigger(ClientSendCommands {
            commands: Some(vec![
                LockstepCommand {
                    command_type_id: *cmd_types.get_id("move".to_string()).unwrap(),
                    ..default()
                }
            ]),
            issued_tick: **tick,
        });
    }
}


