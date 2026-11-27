//! Outil `status` : l'état de l'agent résident.

use crate::residents::ctx::Ctx;
use litemcp::prelude::*;
use litemcp::zod;
use std::sync::Arc;

const DESCRIPTION: &str = "État de cet agent : idle / busy, longueur de la file, tour en cours \
    (qui demande, depuis quand), session, dernier tour.";

pub fn register(builder: ServerBuilder, ctx: Arc<Ctx>) -> ServerBuilder {
    builder
        .tool_parse("status", DESCRIPTION, zod::object().strict(), move |_: serde_json::Value| {
            Ok(ctx.status_json())
        })
        .annotate("status", ToolAnnotations::new().title("État de l'agent").read_only(true))
}
