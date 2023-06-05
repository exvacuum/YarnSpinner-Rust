use crate::prelude::*;
use crate::UnderlyingYarnCommand;
use bevy::prelude::*;

pub(crate) fn dialogue_runner_events_plugin(app: &mut App) {
    app.add_event::<PresentLineEvent>()
        .add_event::<PresentOptionsEvent>()
        .add_event::<ExecuteCommandEvent>()
        .add_event::<NodeCompleteEvent>()
        .add_event::<NodeStartEvent>()
        .add_event::<LineHintsEvent>()
        .add_event::<DialogueCompleteEvent>()
        .add_event::<DialogueStartEvent>();
}

#[derive(Debug, Clone, PartialEq)]
pub struct PresentLineEvent {
    pub line: LocalizedLine,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PresentOptionsEvent {
    pub options: Vec<DialogueOption>,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecuteCommandEvent {
    pub command: UnderlyingYarnCommand,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeCompleteEvent {
    pub node_name: String,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeStartEvent {
    pub node_name: String,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineHintsEvent {
    pub line_ids: Vec<LineId>,
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialogueStartEvent {
    pub source: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialogueCompleteEvent {
    pub source: Entity,
}