# Marketplace

The marketplace is a reader for registry indexes you add. There is no bundled
catalog, no seeded demo plugin and no placeholder listing: with no registry
configured the page says so and stays empty.

## Registry format

A registry is one JSON document, fetched over HTTPS (or `http` on loopback while
developing), at most 512 KiB, with at most 500 listings:

```json
{
  "schemaVersion": 1,
  "name": "Example plugins",
  "plugins": [
    {
      "id": "hello-studio",
      "name": "Hello Studio",
      "version": "0.1.0",
      "description": "Two read-only tools and a JSON formatter panel.",
      "author": "Sarthi Studio",
      "category": "productivity",
      "tags": ["json", "example"],
      "permissions": [],
      "requiresStudio": "4.0.0",
      "downloadUrl": "https://example.com/plugins/hello-studio-0.1.0.zip",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `id` | yes | 1–40 chars of `a-z`, `0-9`, `-`, `_` |
| `name` | yes | ≤ 80 chars |
| `version` | yes | three numeric parts |
| `description` | no | ≤ 400 chars |
| `author` | no | ≤ 80 chars |
| `category` | no | ≤ 40 chars, lowercased when read |
| `tags` | no | ≤ 8, each ≤ 24 chars |
| `permissions` | no | ≤ 16 declared permissions, shown before install |
| `requiresStudio` | no | oldest app version that works |
| `kind` | no | `extension` (default) or `skill` |
| `downloadUrl` | yes | `https`, or `http` on loopback |
| `sha256` | yes | 64 hex characters, lowercase when compared |

`schemaVersion` is optional and means `1`; any other number is refused instead of
being read partially. A malformed listing is named in the page's error list and
skipped; it does not hide the listings that are valid.

## Archive shape

A zip holding either the plugin folder at the root, or exactly one top-level
folder that contains `manifest.json`:

```text
hello-studio.zip
  hello-studio/
    manifest.json
    echo.wasm
    json.html
```

More than one plugin per archive is refused. The manifest's `id` must equal the
listing's `id`.

## Install flow

1. Fetch every configured registry; keep per-registry errors beside the listings
   that loaded.
2. Check the app's `fs_write` rule for the destination folder.
3. Refuse unknown ids, non-extension kinds (skills install from the Skills page)
   and a listing for a newer Studio.
4. Download the archive (max 16 MiB), compute SHA-256, compare with the listing.
5. Extract with traversal, count and size checks in a temporary folder.
6. Verify the manifest id, then install through the same validating path as a
   manual folder install.
7. Record the install in the operational log and refresh the catalog.

## Managing registries

- **Add** — `POST /api/plugins/marketplace/registries` with `{"url": "..."}`.
  Adding the same URL twice keeps one entry; at most 16 registries.
- **Remove** — `POST /api/plugins/marketplace/registries/remove` with the URL.
  Installed plugins stay installed.
- **Browse** — `GET /api/plugins/marketplace` returns `registries`, `entries`
  (validated listings with `registry` on each) and `errors` (one per problem).

Registries live in `plugin-registries.json` next to the app config.

## What the page shows

For each listing: name, description, id, version, author, category, declared
permissions and the Studio version it needs. **Install** downloads and verifies
it. Search filters across id, name, author, category and tags. No counter, rating
or download figure is invented: nothing is shown that a registry did not state.

## Update management

An installed plugin is a folder. Installing a newer version of the same id is
refused while the old folder exists, because overwriting a folder a person may
have edited is not something an install should decide. Remove the old one first;
the removed folder is kept under `.removed/` and can be restored by moving it
back. Automatic plugin updates are deliberately not implemented.
