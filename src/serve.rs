use rmcp::ServerHandler;

use crate::Result;

/// The generic stdio serve seam. Reproduces the exact shape Phase 0's spike
/// proved (`examples/spike.rs`): `handler.serve((stdin, stdout)).await` then
/// `service.waiting().await`. Phase 2 fills in the production stdio lifecycle
/// and the logging-discipline helper that keeps stdout JSON-RPC-only.
pub async fn serve<H: ServerHandler + Send + 'static>(handler: H) -> Result<()> {
    log::debug!("serve: handler type={}", std::any::type_name::<H>());
    let _ = handler;
    todo!("Phase 2: production stdio lifecycle + logging-discipline helper")
}
