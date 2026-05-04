---
"winthorpe": patch
---

Fix the in-app "Connect GitHub" button failing on the released installer with `GitHub account connection is not configured`. The release pipeline now bakes the GitHub OAuth client ID into the binary correctly, and the build refuses to produce a release binary if it isn't configured.
