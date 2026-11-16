# Organization and project access

Status: proposed. Date: 2026-11-15.

## Decision

Use organization memberships with owner/admin/member roles and project grants to users or teams with viewer/contributor/maintainer roles. Projects can be shared by multiple teams. Owners/admins can manage and read the organization's company-managed projects.

Use an external OIDC identity provider for people and expiring scoped tokens for automation. Configure one exact initial owner subject during empty-installation bootstrap. The CLI proposal uses external-browser Authorization Code/PKCE.

## Rationale

Organizations are the primary product audience. Team membership and project access should be explicit instead of assuming every session belongs to a private single-user archive or to one rigid team hierarchy.

## Consequences

Every data surface and background job carries organization/project scope. Contributor attribution does not automatically grant administration. Personal work stays outside configured company collection mappings. Validate the initial identity-provider integration and revisit the administrator visibility policy during product review.
