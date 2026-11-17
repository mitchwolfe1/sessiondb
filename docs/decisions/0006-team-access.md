# Organization and project access

Status: superseded by [0008](0008-collection-and-project-associations.md) and [0013](0013-instance-api-key.md). Date: 2026-11-15.

This record preserves the earlier proposal. Current instance-key access is specified in [access control](../access-control.md).

## Decision

Use organization memberships with owner/admin/member roles and project grants to users or teams with viewer/contributor/maintainer roles. Projects can be shared by multiple teams. Owners/admins can manage and read the organization's company-managed projects.

Use an external OIDC identity provider for people and expiring scoped tokens for automation. Configure one exact initial owner subject during empty-installation bootstrap. The CLI proposal uses external-browser Authorization Code/PKCE.

## Rationale

Shared deployments need a clear access model. Memberships and project grants support both individual and shared collections, with projects accessible to more than one team. [0007](0007-capability-focused-documentation.md) defines the capability-first presentation of these features.

## Consequences

Every data surface and background job carries organization/project scope. Contributor attribution does not automatically grant administration. Personal work stays outside configured company collection mappings. Validate the initial identity-provider integration and revisit the administrator visibility policy during product review.
