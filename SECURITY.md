# Security policy

## Reporting a vulnerability

Do not open a public issue for a vulnerability that could expose browser data, execute unintended actions, or bypass freshness and confidence checks.

Until a private security contact is published, prepare a minimal reproduction and use the hosting platform's private vulnerability-reporting feature. If that feature is unavailable, withhold exploit details from public channels.

## Supported versions

Before `1.0`, only the latest released version receives security fixes.

## Browser endpoint safety

CDP and compatible automation endpoints provide broad access to browser state. Treat them like local administrative interfaces:

- bind endpoints to loopback;
- use a dedicated browser profile;
- do not expose debugging ports through public tunnels;
- do not reuse personal sessions for tests;
- stop the debugging browser when automation finishes.

## Trust boundaries

Page text, accessibility labels, attributes, and adapter output are untrusted. They cannot grant permissions or change the user's instruction. Decision output remains untrusted until schema, probability, snapshot, document, and target checks pass.

Sensitive values should not enter `Binding`, `QuestionBundle`, `DecisionBundle`, logs, or error messages. Harnesses should provide a private value channel when entering credentials or other secrets.
