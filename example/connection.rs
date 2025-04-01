use bevy::prelude::*;
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use bevy_replicon_renet::{
    netcode::{ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication, ServerConfig},
    renet::{ConnectionConfig, RenetClient, RenetServer},
    RenetChannelsExt,
};
use bevy_replicon_lockstep::prelude::*;
use std::{
   env, error::Error, net::{Ipv4Addr, SocketAddr, UdpSocket}, time::SystemTime
};



#[derive(Event)] pub struct TriggerStartServer;
#[derive(Event)] pub struct TriggerStopServer;
#[derive(Event)] pub struct TriggerConnectClient;
#[derive(Event)] pub struct TriggerDisconnectClient;
//#[derive(Event)] pub struct ReconnectClient;

pub(crate) fn start_server (
    _: Trigger<TriggerStartServer>,
    channels: Res<RepliconChannels>,
    mut commands: Commands,
    settings: Res<SimulationSettings>,
    //mut id_map: ResMut<NetworkIdMap>,
) -> Result<(), Box<dyn Error>> {
    let server_channels_config = channels.server_configs();
    let client_channels_config = channels.client_configs();
    let port: u16 = env::var("SERVER_PORT")
                    .expect("SERVER_PORT env var not set")
                    .parse()
                    .expect("Failed to parse SERVER_PORT env var");

    let server = RenetServer::new(ConnectionConfig {
      server_channels_config,
      client_channels_config,
      ..Default::default()
    });

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
      .map_err(|_| "Failed to bind socket on server")?;
    let server_config = ServerConfig {
      current_time,
      max_clients: settings.num_players as usize,
      protocol_id: 0,
      authentication: ServerAuthentication::Unsecure,
      public_addresses: Default::default(),
    };
    let transport = NetcodeServerTransport::new(server_config, socket)?;

    commands.insert_resource(server);
    commands.insert_resource(transport);

    // Local client entity for listen server
    // Remote clients will be sent this information automatically
    // but for a host/client we must set it manually 
    let _host_entity = commands.spawn((
        LocalClient,
        NetworkId::new(1),
    )).id();

    // Cannot add to NetworkIdMap unfortunately
    //*id_map.insert(NetworkId::new(1), host_entity);

    // This step is important.  It tells the server to tell the clients to get 
    // load assets and get ready to play
    commands.set_state(SimulationState::Connecting);
    Ok(())
}
  
pub(super) fn stop_server(
    _: Trigger<TriggerStopServer>,
    mut commands: Commands,
    clients: Query<Entity, With<NetworkId>>,
) {
    commands.remove_resource::<RenetServer>();
    commands.remove_resource::<NetcodeServerTransport>();
    clients.iter().for_each(|client| {
        commands.entity(client).despawn();
    })
}

pub(crate) fn connect_client(
    _: Trigger<TriggerConnectClient>,
    mut commands: Commands,
    channels: Res<RepliconChannels>,
) -> Result<(), Box<dyn Error>> {
    let ip: Ipv4Addr = env::var("SERVER_IP")
        .map_err(|_| "Failed to find SERVER_IP env var")?
        .parse::<Ipv4Addr>()
        .map_err(|_| "Failed to parse SERVER_IP env var")?;
    let port: u16 = env::var("SERVER_PORT")
        .map_err(|_| "Failed to find SERVER_PORT env var")?
        .parse()
        .map_err(|_| "Failed to parse SERVER_PORT env var")?;
    info!("connecting to {ip}:{port}");
    let server_channels_config = channels.server_configs();
    let client_channels_config = channels.client_configs();

    let client = RenetClient::new(ConnectionConfig {
        server_channels_config,
        client_channels_config,
        ..Default::default()
    });

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let client_id = current_time.as_millis() as u64;
    let server_addr = SocketAddr::new(ip.into(), port);
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .map_err(|_| "Failed to bind socket")?;
    let authentication = ClientAuthentication::Unsecure {
        protocol_id: 0,
        client_id,
        server_addr,
        user_data: None,
    };
    let transport = NetcodeClientTransport::new(current_time, authentication, socket)
        .map_err(|_| "Failed to construct client transport")?;

    commands.insert_resource(client);
    commands.insert_resource(transport);
    Ok(())
}

pub(crate) fn disconnect_client(
    _: Trigger<TriggerDisconnectClient>,
    mut commands: Commands,
    clients: Query<Entity, With<NetworkId>>,
) {
    commands.remove_resource::<RenetClient>();
    commands.remove_resource::<NetcodeClientTransport>();
    clients.iter().for_each(|client| {
        commands.entity(client).despawn();
    })
}