use bevy::prelude::*;

use crate::{
    level::*,
    net::{
        message::{FileData, MapHash, Message, MsgType, RequestMap},
        quic::QuicManager,
    },
};

impl Message for MapHash {
    fn handle(self, world: &mut World) {
        let hash = self.0;
        if let Some(compressed) = read_cached_map(&hash) {
            if compressed_level_hash(&compressed).as_deref() == Some(hash.as_str()) {
                crate::messages::push_world(world, "Using cached map.");
                world.insert_resource(PendingMapScene(compressed));
                return;
            }
            crate::messages::push_world(world, "Cached map invalid. Redownloading.");
        }
        crate::messages::push_world(world, "Downloading map...");
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            quic.send(
                crate::net::quic::SendTarget::One(crate::net::quic::SERVER_CONN_ID),
                crate::net::quic::Channel::Ordered,
                &MsgType::RequestMap(RequestMap),
            );
        }
    }
}

impl Message for FileData {
    fn handle(self, world: &mut World) {
        let Self { name, compressed } = self;
        if name == "map.scn.ron" {
            let Some(hash) = compressed_level_hash(&compressed) else {
                eprintln!("FileData: failed to hash map.scn.ron");
                return;
            };
            info!("Client: received map.scn.ron {hash}");
            write_cached_map(&hash, &compressed);
            world.insert_resource(PendingMapScene(compressed));
            return;
        }
        if name != "gametype.lua" {
            return;
        }
        match zstd::stream::decode_all(compressed.as_slice()) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(src) => world.insert_resource(crate::scripting::ScriptConfig {
                    path: String::new(),
                    is_server: false,
                    source: Some(src),
                }),
                Err(e) => eprintln!("FileData: gametype.lua not valid utf8: {e}"),
            },
            Err(e) => eprintln!("FileData: failed to decompress gametype.lua: {e}"),
        };
    }
}
