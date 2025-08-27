# Cedar - Google Docs Local Editor Compatibility Layer

Cedar is a background Rust daemon that enables seamless editing of Google Docs as plain text/Markdown in any local editor (VS Code, Neovim, Emacs) while preserving collaborative features, handling conflicts, and maintaining document integrity.

## Features

- **Secure OAuth2 Authentication** with PKCE flow for desktop applications
- **Real-time Document Synchronization** with conflict detection and resolution
- **Advanced Text Processing** with UTF-8/UTF-16 index mapping for Google Docs compatibility
- **JSON-RPC API** for editor integration with comprehensive error handling
- **Multiple Export Formats** (.docx, .pdf, .md, .txt) via Google Drive API
- **Conflict Resolution** with explicit conflict markers instead of silent overwrites
- **Change Detection** via polling (extensible to webhooks)
- **Configurable Settings** with sensible defaults

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

## Quick Start

### Prerequisites

- Rust (latest stable version)
- Google Cloud Project with Google Docs API and Google Drive API enabled
- OAuth2 credentials configured for a desktop application

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/cedar.git
   cd cedar
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Set up Google Cloud credentials:
   - Go to [Google Cloud Console](https://console.cloud.google.com/)
   - Create a project and enable Google Docs API and Google Drive API
   - Create OAuth2 credentials for a "Desktop Application"
   - Configure the redirect URI as `http://127.0.0.1:8080`

4. Configure Cedar:
   ```bash
   # Cedar will create a default config file on first run
   ./target/release/cedar
   ```
   
   Edit the generated config file at `~/.config/cedar/config.toml`:
   ```toml
   [google]
   client_id = "your-client-id.googleusercontent.com"
   client_secret = "your-client-secret"
   redirect_uri = "http://127.0.0.1:8080"
   scopes = [
       "https://www.googleapis.com/auth/documents",
       "https://www.googleapis.com/auth/drive.readonly"
   ]
   
   [server]
   host = "127.0.0.1"
   port = 3030
   
   [sync]
   poll_interval_seconds = 5
   debounce_milliseconds = 500
   max_retries = 3
   ```

5. Run the Cedar daemon:
   ```bash
   ./target/release/cedar
   ```

## Usage

### JSON-RPC API

Cedar exposes a JSON-RPC 2.0 API on `http://127.0.0.1:3030` with the following methods:

#### `authenticate`
Initiates OAuth2 authentication flow.
```json
{
  "jsonrpc": "2.0",
  "method": "authenticate",
  "id": 1
}
```

#### `open_document`
Opens a Google Doc for editing.
```json
{
  "jsonrpc": "2.0",
  "method": "open_document",
  "params": {
    "document_id": "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
    "suggestions_mode": "inline"
  },
  "id": 2
}
```

#### `sync_document`
Synchronizes local changes with Google Docs.
```json
{
  "jsonrpc": "2.0",
  "method": "sync_document",
  "params": {
    "document_id": "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
    "content": "Updated document content...",
    "revision_id": "ALm37BWWMiX7FWQN_6-2QaXR6ePEt2fz_H1XqnlFT7VVDC_U"
  },
  "id": 3
}
```

#### `export_document`
Exports a document in various formats.
```json
{
  "jsonrpc": "2.0",
  "method": "export_document",
  "params": {
    "document_id": "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
    "format": "docx"
  },
  "id": 4
}
```

### Editor Integration Examples

#### VS Code Extension (TypeScript)
```typescript
import { createConnection, ProposedFeatures } from 'vscode-languageserver';

const connection = createConnection(ProposedFeatures.all);

async function openDocument(documentId: string) {
    const response = await fetch('http://127.0.0.1:3030', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            jsonrpc: '2.0',
            method: 'open_document',
            params: { document_id: documentId },
            id: Date.now()
        })
    });
    
    return response.json();
}
```

#### Neovim Plugin (Lua)
```lua
local cedar = {}

function cedar.open_document(document_id)
    local curl = require('plenary.curl')
    
    local response = curl.post('http://127.0.0.1:3030', {
        headers = { ['Content-Type'] = 'application/json' },
        body = vim.json.encode({
            jsonrpc = '2.0',
            method = 'open_document',
            params = { document_id = document_id },
            id = os.time()
        })
    })
    
    return vim.json.decode(response.body)
end
```

## 🔧 Configuration

### Environment Variables

- `CEDAR_CONFIG_PATH` - Custom config file location
- `CEDAR_LOG_LEVEL` - Log level (trace, debug, info, warn, error)
- `CEDAR_BIND_HOST` - Override server host
- `CEDAR_BIND_PORT` - Override server port

### Suggestions Modes

Cedar supports three Google Docs suggestion modes:

- **`inline`** - Show suggestions inline with the text (default)
- **`accepted`** - Preview with all suggestions accepted
- **`rejected`** - Preview with all suggestions rejected

## Security & Privacy

- **Local-first architecture**: Documents are never stored on Cedar servers
- **OAuth2 + PKCE**: Industry-standard authentication with enhanced security for desktop applications
- **Minimal permissions**: Requests only the necessary Google API scopes
- **Secure token storage**: Refresh tokens are encrypted and stored locally
- **HTTPS enforcement**: All communication with Google APIs uses HTTPS

## Conflict Resolution

Cedar provides explicit conflict resolution instead of silent overwrites:

```markdown
<<<<<<< LOCAL
Your local changes
=======
Remote changes from Google Docs
>>>>>>> REMOTE (original content)
```

## Testing

```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test integration

# Run property-based tests
cargo test --features proptest
```

## Performance

- **Memory usage**: 10-50MB depending on document size
- **Sync latency**: 200-500ms for typical documents
- **Rate limiting**: Automatic handling of Google API quotas with exponential backoff
- **Concurrent documents**: Support for multiple documents simultaneously

## Development

### Project Structure

```
src/
├── main.rs          # Application entry point
├── auth.rs          # OAuth2 authentication with PKCE
├── config.rs        # Configuration management
├── docs.rs          # Google Docs API client
├── drive.rs         # Google Drive API client  
├── diff.rs          # Text diff algorithms with UTF-16 support
├── rpc.rs           # JSON-RPC server implementation
└── errors.rs        # Comprehensive error handling
```

### Key Components

- **Auth Manager**: Handles OAuth2 flow with PKCE, token refresh, and secure storage
- **Docs Client**: Complete Google Docs API wrapper with document model
- **Drive Client**: Google Drive API for change detection and exports  
- **Diff Calculator**: Advanced text diffing with UTF-8/UTF-16 index conversion
- **RPC Server**: JSON-RPC 2.0 compliant server with async support

### Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Run tests: `cargo test`
4. Run lints: `cargo clippy -- -D warnings`
5. Format code: `cargo fmt`
6. Commit changes: `git commit -m 'Add amazing feature'`
7. Push to branch: `git push origin feature/amazing-feature`
8. Open a pull request

## Editor Integrations

### Planned/In Development

- **VS Code Extension** - Full-featured integration with document browser
- **Neovim Plugin** - Lua-based plugin with async support  
- **Emacs Package** - Elisp integration with org-mode compatibility
- **Sublime Text Plugin** - Python-based integration
- **IntelliJ Plugin** - Kotlin/Java integration for JetBrains IDEs

## Roadmap

- [ ] **Push Notifications** - Replace polling with Google Drive webhooks
- [ ] **Offline Support** - Cache documents for offline editing
- [ ] **Collaborative Cursors** - Real-time cursor positions
- [ ] **Comment Integration** - Bidirectional comment sync
- [ ] **Version History** - Access Google Docs revision history
- [ ] **Batch Operations** - Multi-document operations
- [ ] **Plugin Ecosystem** - SDK for custom integrations

## Known Limitations

- **Tables & Images**: Complex formatting elements are treated as placeholders
- **Drawings**: Positioned drawings are not editable in text mode  
- **Advanced Formatting**: Some rich formatting may be lost during conversion
- **Suggestions**: Programmatic accept/reject of suggestions not fully supported
- **Real-time Collaboration**: Limited to change detection, not real-time cursors

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- **Google Workspace APIs** for providing comprehensive document APIs
- **Rust Community** for excellent async and HTTP libraries
- **OAuth2 Crate** for robust authentication implementation
- **Similar Crate** for efficient text diffing algorithms

## Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/cedar/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/cedar/discussions)
- **Documentation**: [Wiki](https://github.com/yourusername/cedar/wiki)

---

Cedar bridges the gap between Google Docs collaboration and local editor productivity.