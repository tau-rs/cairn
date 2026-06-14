//! Verifies the `#[ts(export)]` bindings generate without error.
use cairn_contract::{
    AnswerEvent, AskRequest, Command, CommandResponse, ContractError, Event, GraphEdge,
    NoteSummary, Query, QueryResponse, TagCount,
};
use ts_rs::TS;

#[test]
fn exports_typescript_bindings() {
    assert!(Command::decl().contains("Command"));
    assert!(Query::decl().contains("Query"));
    assert!(Event::decl().contains("Event"));
    assert!(CommandResponse::decl().contains("CommandResponse"));
    assert!(QueryResponse::decl().contains("QueryResponse"));
    assert!(ContractError::decl().contains("ContractError"));
    assert!(NoteSummary::decl().contains("NoteSummary"));
    assert!(GraphEdge::decl().contains("GraphEdge"));
    assert!(TagCount::decl().contains("TagCount"));
    assert!(AskRequest::decl().contains("AskRequest"));
    assert!(AnswerEvent::decl().contains("AnswerEvent"));
    Command::export_all().unwrap();
    Query::export_all().unwrap();
    Event::export_all().unwrap();
    CommandResponse::export_all().unwrap();
    QueryResponse::export_all().unwrap();
    ContractError::export_all().unwrap();
    NoteSummary::export_all().unwrap();
    GraphEdge::export_all().unwrap();
    TagCount::export_all().unwrap();
    AskRequest::export_all().unwrap();
    AnswerEvent::export_all().unwrap();
}
