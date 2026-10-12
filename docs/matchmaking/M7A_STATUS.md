# M7a status: questionnaires and the evidence registry (first slice)

Step one of the psychology and learning milestone (M7). Step two (M7b: outcome data export, versioned scoring configurations, ranking compared with the deterministic baseline) comes next.

## What it does
- **Evidence registry.** A questionnaire is data: items, a response scale, subscales, and an **evidence record**: its source citations, an evidence level, what it *cannot* tell you, and how to read its
  scores. Three levels keep the application honest: *Published, validated instrument* (only questionnaires shipped with Matchwise), *Adapted from a published instrument* (must cite the original; its
  evidence does not carry over), and *Written by the matchmaker (not validated)*. A custom questionnaire can never claim the first level, and must state its limitations before it can be saved.
- **Instruments are versioned and never edited in place**: a new version is a new row, so every response can be read against exactly the questions that produced it. Archiving is respected.
- **Scores become ordinary profile fields** with *questionnaire* provenance (rank: above AI, below anything a person entered). They are numeric and **sensitive** by default, so they stay out of introduction
  cards, out of cloud-AI prompts unless allowed, and are not shown back to candidates. A retake replaces the earlier score (both responses are kept); a value a person entered is never overwritten (reported
  as "not applied"). Deleting a response can also clear the scores it wrote, and only those.
- **Shipped with the app:** the 20-item **Mini-IPIP** (Big Five; Donnellan, Oswald, Baird & Lucas, 2006; items from the public-domain IPIP) filling five new fields (4-20 per trait).
- **Custom questionnaires:** a builder (name, dimension it informs, 2-7 point scale, scores with sum or average, questions with reverse scoring, evidence record). Each score creates a numeric field automatically.
- **Two ways to answer.** In the app, the matchmaker enters answers during an interview (progress is saved after every answer). On **Telegram**, a candidate sends `/questionnaires`: one question per
  message with a button per scale point, resumable, and the candidate is told only that they finished: **scores are never shown to the candidate**. Assigning a questionnaire to a linked person notifies them.
- **The scorecard stays honest.** Personality, values and communication show "not assessed" until a rule uses the data. The starter rule set contains two **switched-off** personality example rules, labelled with
  the strength of the evidence, so nothing is scored on personality unless the matchmaker turns it on.

## What the evidence says (my summary, with the sources in the registry)
Studies of couples find that similarity between partners' trait scores predicts satisfaction very little beyond each person's own traits (Dyrenforth et al., 2010), and that self-reported individual traits gathered
before a relationship explain only a small part of relationship quality, with relationship-specific variables doing better (Joel et al., 2020). The registry says this next to the instrument and on the questionnaires
page. **Please check these citations and my reading of them before quoting them to anyone**; they are in the instrument's evidence record, which you can edit by adding your own custom entry.

## Be careful with
- **The Mini-IPIP item wording and keying were reproduced from memory** of the published scale (the record says so). Compare them with Donnellan et al. (2006) or ipip.ori.org before relying on the scores.
- Four items per trait is a rough picture. There are no norms or percentiles: scores are raw sums, 4-20, with the midpoint 12.
- English only; no translation, and no cross-cultural calibration.
- Other validated instruments (attachment, values, communication) are not shipped: many are copyrighted and I will not reproduce one I cannot cite reliably. Add a questionnaire as *adapted* with its citation if you hold a licence.

## Verified here
7 in-crate tests (registry and seeding, answering in the app, provenance and non-overwrite, versions, cascade deletion, and the Telegram flow including stale, skipped, foreign and out-of-range buttons, resuming,
and that candidates are never shown scores) plus 8 core tests (scoring with reverse keys, partial answers, validation, round trip). The commands ran end to end (including the rules that stop a custom questionnaire
claiming validation or editing a shipped one). The Rust JSON matches the TypeScript types. 70 core tests and 45 app-layer tests pass in my sandbox.

## Not verified / limits
- Not compiled in the Tauri build; the UI was type-checked and its helper logic exercised, not clicked through.
- No automated tests of the React builder form.
- Questionnaire completion by a candidate is not itself consent to psychological profiling beyond the Telegram privacy notice: if you work somewhere with stricter rules, add your own consent wording.
