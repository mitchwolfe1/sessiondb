# On-demand access to session history

Status: accepted product direction.

Agents fetch history from SessionDB when a previous decision or procedure could change their next action. Search returns excerpts linked to original messages. Agents can open those messages on the recorded branch, check who said what and cite the evidence in their answers.

The CLI, SDK and harness integrations use the public SessionDB API. For cross-session questions, the calling agent can compare independent exchanges, look for exceptions and report gaps in coverage.

Planned timelines and briefings will link back to the underlying exchanges. Reviewed memories will retain their scope, review status and supporting message references. See the [delivery plan](../roadmap.md#timelines-briefings-and-reviewed-memories).
