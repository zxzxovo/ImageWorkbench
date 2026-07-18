# Privacy and data handling

ImageWorkbench is local-first. Project databases, imported inputs, generated
outputs, prompts, and provider responses are kept in the project directory.
Provider API keys are stored in the operating-system credential manager. A
generation request and any reference images are sent only to the provider the
user selected.

Diagnostics and support bundles must be reviewed before sharing: they may
contain paths, model names, request IDs, and error text. API keys are redacted,
but generated content and provider-specific metadata may still reveal private
information. The optional updater contacts the configured GitHub release
endpoint and requires confirmation before downloading and installing an
update.
