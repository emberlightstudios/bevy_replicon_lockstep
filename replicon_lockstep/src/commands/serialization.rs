use bevy::{prelude::*, reflect::serde::{ReflectDeserializer, ReflectSerializer}, utils::hashbrown::HashMap};
use bevy_replicon::{
    bytes::Bytes,
    postcard::{
        self, Deserializer, Error, Serializer
    },
    shared::{
        event::ctx::{ClientReceiveCtx, ClientSendCtx, ServerReceiveCtx, ServerSendCtx},
        postcard_utils::{BufFlavor, ExtendMutFlavor}
    }
};
use serde::{Serialize, Deserialize, de::DeserializeSeed};
use super::{
    ClientSendCommands,
    LockstepClientCommands,
    ServerSendCommands
};

use crate::prelude::SimTick;

pub(super) fn serialize_client_send_commands(
    ctx: &mut ClientSendCtx,
    event: &ClientSendCommands,
    message: &mut Vec<u8>,
) -> postcard::Result<()> {
    let mut serializer = Serializer {
        output: ExtendMutFlavor::new(message),
    };
    (event.commands.len() as u16).serialize(&mut serializer)?;
    for command in &event.commands {
        ReflectSerializer::new(&*command.as_partial_reflect(), ctx.type_registry)
            .serialize(&mut serializer)?;
    }
    event.issued_tick.serialize(&mut serializer)?;
    Ok(())
}

pub(super) fn deserialize_client_send_commands(
    ctx: &mut ServerReceiveCtx,
    message: &mut Bytes,
) -> postcard::Result<ClientSendCommands> {
 let mut deserializer = Deserializer::from_flavor(BufFlavor::new(message));
    let num_commands = u16::deserialize(&mut deserializer)? as usize;
    let mut commands = Vec::with_capacity(num_commands);

    for _ in 0..num_commands {
        let reflect_deserializer = ReflectDeserializer::new(ctx.type_registry);
        let payload = reflect_deserializer.deserialize(&mut deserializer)?
            .as_partial_reflect()
            .clone_value();

        commands.push(payload);
    }
    let issued_tick = SimTick::deserialize(&mut deserializer)?;
    Ok(ClientSendCommands { commands, issued_tick })
}

pub(super) fn serialize_server_send_commands(
    ctx: &mut ServerSendCtx,
    event: &ServerSendCommands,
    message: &mut Vec<u8>,
) -> postcard::Result<()> {
    let mut serializer = Serializer {
        output: ExtendMutFlavor::new(message),
    };
    (event.commands.len() as u8).serialize(&mut serializer)?;
    for (client_id, commands) in event.commands.iter() {
        client_id.serialize(&mut serializer)?;
        (commands.len() as u16).serialize(&mut serializer)?;
        for command in commands {
            ReflectSerializer::new(&*command.as_partial_reflect(), ctx.type_registry)
                .serialize(&mut serializer)?
        }
    }
    event.tick.serialize(&mut serializer)?;
    Ok(())
}

pub(super) fn deserialize_server_send_commands(
    ctx: &mut ClientReceiveCtx,
    message: &mut Bytes,
) -> postcard::Result<ServerSendCommands> {
    let mut deserializer = Deserializer::from_flavor(BufFlavor::new(message));

    // Deserialize the number of commands
    let num_clients = u8::deserialize(&mut deserializer)?;
    let mut client_commands: HashMap<u64, Vec<_>> = HashMap::<u64, Vec<_>>::with_capacity(num_clients.into());
    for _ in 0..num_clients {
        let client_id = u64::deserialize(&mut deserializer)?;
        let num_commands = u16::deserialize(&mut deserializer)?;
        let mut commands: Vec<Box<dyn PartialReflect>> = Vec::<_>::with_capacity(num_commands as usize);
        for __ in 0..num_commands {
            let reflect_deserializer = ReflectDeserializer::new(ctx.type_registry);
            let payload = reflect_deserializer.deserialize(&mut deserializer)?
                .as_partial_reflect()
                .clone_value();
            commands.push(payload);
        }
        client_commands.insert(client_id, commands);
    }
    let tick: u32 = SimTick::deserialize(&mut deserializer)?;
    Ok(ServerSendCommands { commands: LockstepClientCommands(client_commands), tick })
}
