use super::*;
use prost::Message;

pub struct ChannelBuffer {
    pub base_text: String,
    pub operations: Vec<proto::Operation>,
}

impl Database {
    pub async fn update_buffer(
        &self,
        buffer_id: BufferId,
        operations: &[proto::Operation],
    ) -> Result<()> {
        self.transaction(|tx| async move {
            let buffer = buffer::Entity::find_by_id(buffer_id)
                .one(&*tx)
                .await?
                .ok_or_else(|| anyhow!("no such buffer"))?;
            buffer_operation::Entity::insert_many(operations.iter().filter_map(|operation| {
                match operation.variant.as_ref()? {
                    proto::operation::Variant::Edit(operation) => {
                        let value =
                            serialize_edit_operation(&operation.ranges, &operation.new_text);
                        let version = serialize_version(&operation.version);
                        Some(buffer_operation::ActiveModel {
                            buffer_id: ActiveValue::Set(buffer_id),
                            epoch: ActiveValue::Set(buffer.epoch),
                            replica_id: ActiveValue::Set(operation.replica_id as i32),
                            lamport_timestamp: ActiveValue::Set(operation.lamport_timestamp as i32),
                            local_timestamp: ActiveValue::Set(operation.local_timestamp as i32),
                            is_undo: ActiveValue::Set(false),
                            version: ActiveValue::Set(version),
                            value: ActiveValue::Set(value),
                        })
                    }
                    proto::operation::Variant::Undo(operation) => {
                        let value = serialize_undo_operation(&operation.counts);
                        let version = serialize_version(&operation.version);
                        Some(buffer_operation::ActiveModel {
                            buffer_id: ActiveValue::Set(buffer_id),
                            epoch: ActiveValue::Set(buffer.epoch),
                            replica_id: ActiveValue::Set(operation.replica_id as i32),
                            lamport_timestamp: ActiveValue::Set(operation.lamport_timestamp as i32),
                            local_timestamp: ActiveValue::Set(operation.local_timestamp as i32),
                            is_undo: ActiveValue::Set(true),
                            version: ActiveValue::Set(version),
                            value: ActiveValue::Set(value),
                        })
                    }
                    proto::operation::Variant::UpdateSelections(_) => None,
                    proto::operation::Variant::UpdateDiagnostics(_) => None,
                    proto::operation::Variant::UpdateCompletionTriggers(_) => None,
                }
            }))
            .exec(&*tx)
            .await?;

            Ok(())
        })
        .await
    }

    pub async fn join_buffer_for_channel(
        &self,
        channel_id: ChannelId,
        user_id: UserId,
        connection: ConnectionId,
    ) -> Result<ChannelBuffer> {
        self.transaction(|tx| async move {
            let tx = tx;

            // Get or create buffer from channel
            self.check_user_is_channel_member(channel_id, user_id, &tx)
                .await?;

            let buffer = channel::Model {
                id: channel_id,
                ..Default::default()
            }
            .find_related(buffer::Entity)
            .one(&*tx)
            .await?;

            let buffer = if let Some(buffer) = buffer {
                buffer
            } else {
                let buffer = buffer::ActiveModel {
                    channel_id: ActiveValue::Set(channel_id),
                    ..Default::default()
                }
                .insert(&*tx)
                .await?;
                buffer
            };

            // Join the collaborators
            let collaborators = buffer
                .find_related(channel_buffer_collaborator::Entity)
                .all(&*tx)
                .await?;
            let replica_ids = collaborators
                .iter()
                .map(|c| c.replica_id)
                .collect::<HashSet<_>>();
            let mut replica_id = ReplicaId(0);
            while replica_ids.contains(&replica_id) {
                replica_id.0 += 1;
            }
            channel_buffer_collaborator::ActiveModel {
                buffer_id: ActiveValue::Set(buffer.id),
                connection_id: ActiveValue::Set(connection.id as i32),
                connection_server_id: ActiveValue::Set(ServerId(connection.owner_id as i32)),
                user_id: ActiveValue::Set(user_id),
                replica_id: ActiveValue::Set(replica_id),
                ..Default::default()
            }
            .insert(&*tx)
            .await?;

            // Assemble the buffer state
            let id = buffer.id;
            let base_text = if buffer.epoch > 0 {
                buffer_snapshot::Entity::find()
                    .filter(
                        buffer_snapshot::Column::BufferId
                            .eq(id)
                            .and(buffer_snapshot::Column::Epoch.eq(buffer.epoch)),
                    )
                    .one(&*tx)
                    .await?
                    .ok_or_else(|| anyhow!("no such snapshot"))?
                    .text
            } else {
                String::new()
            };

            let mut rows = buffer_operation::Entity::find()
                .filter(
                    buffer_operation::Column::BufferId
                        .eq(id)
                        .and(buffer_operation::Column::Epoch.eq(buffer.epoch)),
                )
                .stream(&*tx)
                .await?;
            let mut operations = Vec::new();
            while let Some(row) = rows.next().await {
                let row = row?;
                let version = deserialize_version(&row.version)?;
                let operation = if row.is_undo {
                    let counts = deserialize_undo_operation(&row.value)?;
                    proto::operation::Variant::Undo(proto::operation::Undo {
                        replica_id: row.replica_id as u32,
                        local_timestamp: row.local_timestamp as u32,
                        lamport_timestamp: row.lamport_timestamp as u32,
                        version,
                        counts,
                    })
                } else {
                    let (ranges, new_text) = deserialize_edit_operation(&row.value)?;
                    proto::operation::Variant::Edit(proto::operation::Edit {
                        replica_id: row.replica_id as u32,
                        local_timestamp: row.local_timestamp as u32,
                        lamport_timestamp: row.lamport_timestamp as u32,
                        version,
                        ranges,
                        new_text,
                    })
                };
                operations.push(proto::Operation {
                    variant: Some(operation),
                })
            }

            Ok(ChannelBuffer {
                base_text,
                operations,
            })
        })
        .await
    }

    pub async fn get_buffer_collaborators(&self, buffer: BufferId) -> Result<()> {
        todo!()
    }

    pub async fn leave_buffer(&self, buffer: BufferId, user: UserId) -> Result<()> {
        self.transaction(|tx| async move {
            //TODO
            // let tx = tx;
            // let channel = channel::Entity::find_by_id(channel_id)
            //     .one(&*tx)
            //     .await?
            //     .ok_or_else(|| anyhow!("invalid channel"))?;

            // if let Some(id) = channel.main_buffer_id {
            //     return Ok(id);
            // } else {
            //     let buffer = buffer::ActiveModel::new().insert(&*tx).await?;
            //     channel::ActiveModel {
            //         id: ActiveValue::Unchanged(channel_id),
            //         main_buffer_id: ActiveValue::Set(Some(buffer.id)),
            //         ..Default::default()
            //     }
            //     .update(&*tx)
            //     .await?;
            //     Ok(buffer.id)
            // }
            Ok(())
        })
        .await
    }
}

mod storage {
    #![allow(non_snake_case)]

    use prost::Message;

    pub const VERSION: usize = 1;

    #[derive(Message)]
    pub struct VectorClock {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<VectorClockEntry>,
    }

    #[derive(Message)]
    pub struct VectorClockEntry {
        #[prost(uint32, tag = "1")]
        pub replica_id: u32,
        #[prost(uint32, tag = "2")]
        pub timestamp: u32,
    }

    #[derive(Message)]
    pub struct TextEdit {
        #[prost(message, repeated, tag = "1")]
        pub ranges: Vec<Range>,
        #[prost(string, repeated, tag = "2")]
        pub texts: Vec<String>,
    }

    #[derive(Message)]
    pub struct Range {
        #[prost(uint64, tag = "1")]
        pub start: u64,
        #[prost(uint64, tag = "2")]
        pub end: u64,
    }

    #[derive(Message)]
    pub struct Undo {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<UndoCount>,
    }

    #[derive(Message)]
    pub struct UndoCount {
        #[prost(uint32, tag = "1")]
        pub replica_id: u32,
        #[prost(uint32, tag = "2")]
        pub local_timestamp: u32,
        #[prost(uint32, tag = "3")]
        pub count: u32,
    }
}

fn serialize_version(version: &Vec<proto::VectorClockEntry>) -> Vec<u8> {
    storage::VectorClock {
        entries: version
            .iter()
            .map(|entry| storage::VectorClockEntry {
                replica_id: entry.replica_id,
                timestamp: entry.timestamp,
            })
            .collect(),
    }
    .encode_to_vec()
}

fn deserialize_version(bytes: &[u8]) -> Result<Vec<proto::VectorClockEntry>> {
    let clock = storage::VectorClock::decode(bytes).map_err(|error| anyhow!("{}", error))?;
    Ok(clock
        .entries
        .into_iter()
        .map(|entry| proto::VectorClockEntry {
            replica_id: entry.replica_id,
            timestamp: entry.timestamp,
        })
        .collect())
}

fn serialize_edit_operation(ranges: &[proto::Range], texts: &[String]) -> Vec<u8> {
    storage::TextEdit {
        ranges: ranges
            .iter()
            .map(|range| storage::Range {
                start: range.start,
                end: range.end,
            })
            .collect(),
        texts: texts.to_vec(),
    }
    .encode_to_vec()
}

fn deserialize_edit_operation(bytes: &[u8]) -> Result<(Vec<proto::Range>, Vec<String>)> {
    let edit = storage::TextEdit::decode(bytes).map_err(|error| anyhow!("{}", error))?;
    let ranges = edit
        .ranges
        .into_iter()
        .map(|range| proto::Range {
            start: range.start,
            end: range.end,
        })
        .collect();
    Ok((ranges, edit.texts))
}

fn serialize_undo_operation(counts: &Vec<proto::UndoCount>) -> Vec<u8> {
    storage::Undo {
        entries: counts
            .iter()
            .map(|entry| storage::UndoCount {
                replica_id: entry.replica_id,
                local_timestamp: entry.local_timestamp,
                count: entry.count,
            })
            .collect(),
    }
    .encode_to_vec()
}

fn deserialize_undo_operation(bytes: &[u8]) -> Result<Vec<proto::UndoCount>> {
    let undo = storage::Undo::decode(bytes).map_err(|error| anyhow!("{}", error))?;
    Ok(undo
        .entries
        .iter()
        .map(|entry| proto::UndoCount {
            replica_id: entry.replica_id,
            local_timestamp: entry.local_timestamp,
            count: entry.count,
        })
        .collect())
}
