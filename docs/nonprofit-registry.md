# White Warren for nonprofits: profiles and Ratified

Status: design draft, 2026-10-14. Nothing here is built yet.

White Warren helps Canadian nonprofits keep their data in a file they control.
This note adds a public side: a voluntary registry of nonprofit profiles, and
a voluntary **Ratified** label for organizations that meet basic standards.

It reuses the approach of RATi Trust (trust.rati.chat): standards published as
data, sealed versions, four result states, and cases we learn from. It does not
share that site or its domain, because that site rates memecoins and stocks.

## Two layers

| Layer | Who decides | Public? |
| --- | --- | --- |
| **Assessment.** We gather public facts and check them against the standards: registry records, DNS, the organization's website. | We run it automatically. | No. Only the organization sees its results. |
| **Ratified.** The organization reviews its results and accepts them. | The organization. | Only if it accepts. |

An organization that never claims its profile has no public page. The facts we
gather pre-fill a draft, so claiming takes minutes instead of a form.

## Result states

Same as RATi Trust:

- **Met.** We read the fact and the standard holds.
- **Not met.** We read the fact and the standard does not hold.
- **Not checked yet.** We could not read the fact. This blocks Ratified until
  the organization sends evidence we can check.
- **Not applied.** The standard does not fit this kind of organization, for
  example financial filings for a nonprofit that is not a charity. Shown with
  the reason and left out of the count.

Ratified means every applied standard is met. It is not an endorsement, a
guarantee, advice or an audit opinion.

## Standards v0.1 (draft)

Organization:

1. Active in its corporate registry (Corporations Canada or the province).
2. Annual return filed on time.
3. If it says it is a registered charity, the CRA list agrees.
4. A registered charity filed its last T3010 on time. (Not applied for other
   nonprofits.)

Website and email:

5. The website uses HTTPS with a valid certificate.
6. The domain has an enforcing DMARC policy, so others cannot easily send email
   as the organization.
7. The website publishes a privacy policy and a way to contact the organization.
8. The website loads no third-party tracking that its privacy or cookie notice
   does not disclose.
9. Pages where people ask for help load no advertising or social-media trackers.
10. The website meets basic accessibility checks (page language, alt text,
    form labels, contrast).

Each standard will list its exact test, its data source and how often it is
re-read, the same way `rules.toml` does in RATi Trust.

Every "not met" is something we can help fix. That is the link to our
nonprofit setup kit: assess, fix, then ratify.

## The profile

The organization writes its profile in White Warren: mission, services,
regions, languages, who it serves, how to refer someone, and optionally its
funders and financial statements. It publishes the profile as a signed file on
its own domain (`did:web`). The registry only indexes published profiles.

Each claim in a profile is shown as **verified** (matched to a public record,
with the source and date) or **self-reported**. A claim that does not match a
record is shown to the organization only.

The same profile feeds grant matching (grant-watch) on the organization's own
machine, so it never has to be uploaded.

## Claiming a profile

Two proofs, because people who harass victim-support groups have reasons to
pose as them:

1. **Control of the domain:** a DNS TXT record or the published `did:web` file.
2. **Authority to act:** the person is a director in the corporate registry, or
   the board approves them in a short signed resolution.

## Privacy and safety

- **No human names, ever.** Profiles show organizations, not people: no
  directors, staff, volunteers or contacts by name, even when a registry lists
  them. Each profile links to the organization's own website for more.
- We collect from organizations' websites and public records, and keep only the
  facts a standard or the profile needs. We do not store personal information.
- Director names from a corporate registry are used only in private, to check
  that the person claiming a profile can act for the organization. They are
  never shown.
- Organizations that serve victims can also hide addresses and intake details,
  and show only a way to refer someone.
- We never publish a shelter or office location unless the organization
  chooses to.
- An organization can withdraw at any time. Its results are deleted. We keep
  only that a label was issued or withdrawn, and when.
- Published standards versions stay public forever.

## First users

1. RATi OSF, assessed and published first, including anything we fail.
2. Cyber Harassment Support, if Varsha wants it. Her private review comes
   first and is a separate conversation.

## Open questions

- Domain and name for the public registry.
- Whether the profile format should follow an existing standard (for example
  schema.org `NGO`) as well as `did:web`.
- How often to re-check, and how long a label lasts before re-acceptance.
- Who besides RATi OSF may run assessments, and how results are signed.
