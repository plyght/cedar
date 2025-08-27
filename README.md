# Cedar

A Rust daemon that enables local editor access to Google Docs with real-time sync and conflict resolution.

## Features

- OAuth2 authentication with PKCE
- Real-time document synchronization
- UTF-8/UTF-16 index mapping for Google Docs compatibility  
- JSON-RPC API for editor integration
- Export formats: .docx, .pdf, .md, .txt
- Explicit conflict resolution with merge markers
- Change detection via polling

## Architecture

```
┌─────────────────┐    JSON-RPC    ┌─────────────────┐    OAuth2/HTTPS    ┌─────────────────┐
│   VS Code       │ ◄────────────► │   Cedar Daemon  │ ◄─────────────────► │  Google APIs    │
│   Neovim        │                │                 │                     │  • Docs API     │
│   Emacs         │                │  • Auth Manager │                     │  • Drive API    │
│   Any Editor    │                │  • Diff Engine  │                     │                 │
└─────────────────┘                │  • Sync Engine  │                     └─────────────────┘
                                   │  • RPC Server   │
                                   └─────────────────┘
```

## Setup

**Prerequisites**: Rust, Google Cloud Project with Docs/Drive APIs enabled, OAuth2 desktop credentials

1. **Build**: `cargo build --release`
2. **Google Cloud Setup**:
   - Enable Google Docs API and Google Drive API
   - Create OAuth2 credentials (Desktop Application)
   - Set redirect URI: `http://127.0.0.1:8080`
3. **Configure** `~/.config/cedar/config.toml`:
   ```toml
   [google]
   client_id = "your-client-id.googleusercontent.com"
   client_secret = "your-client-secret"
   redirect_uri = "http://127.0.0.1:8080"
   scopes = ["https://www.googleapis.com/auth/documents", "https://www.googleapis.com/auth/drive.readonly"]
   
   [server]
   host = "127.0.0.1"
   port = 3030
   ```
4. **Run**: `./target/release/cedar`

## API

JSON-RPC 2.0 server at `http://127.0.0.1:3030`

**Core Methods**:
- `authenticate` - Start OAuth2 flow
- `open_document(document_id, suggestions_mode)` - Open doc for editing
- `sync_document(document_id, content, revision_id)` - Sync local changes
- `export_document(document_id, format)` - Export to .docx/.pdf/.md/.txt

## Integration Example

```typescript
// VS Code/TypeScript
async function openDoc(docId: string) {
    return fetch('http://127.0.0.1:3030', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({
            jsonrpc: '2.0', method: 'open_document',
            params: {document_id: docId}, id: 1
        })
    }).then(r => r.json());
}
```

## Configuration

**Environment Variables**:
- `CEDAR_CONFIG_PATH` - Custom config location
- `CEDAR_LOG_LEVEL` - Log verbosity
- `CEDAR_BIND_HOST/PORT` - Server overrides

**Suggestion Modes**: `inline` (default), `accepted`, `rejected`

## Security

- Local-first: no server storage
- OAuth2 + PKCE authentication
- Minimal API permissions
- Encrypted token storage
- HTTPS enforcement

## Conflict Resolution

```markdown
<<<<<<< LOCAL
Your local changes
=======
Remote changes from Google Docs
>>>>>>> REMOTE
```

## Development

```bash
# Test
cargo test
cargo test --test integration
cargo test --features proptest

# Quality
cargo clippy -- -D warnings
cargo fmt
```

**Structure**: `src/{main,auth,config,docs,drive,diff,rpc,errors}.rs`

## Limitations

- Tables/images become placeholders
- Some rich formatting lost in conversion
- No real-time cursors (polling-based sync)
- Limited programmatic suggestion handling

## Roadmap

- [ ] Webhook push notifications
- [ ] Offline document caching  
- [ ] Comment integration
- [ ] Version history access

## License

MIT - see [LICENSE](LICENSE)