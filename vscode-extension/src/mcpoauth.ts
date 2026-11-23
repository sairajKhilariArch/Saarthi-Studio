// OAuth sign-in for MCP servers in the VS Code built-in server.
//
// Ported from app/src/mcp_oauth.rs, so both apps authorize the same way:
//
//   * discovery follows MCP: a protected-resource document (RFC 9728) names
//     the authorization server, and that server's metadata (RFC 8414) must
//     claim the issuer it was found at — a document found elsewhere that
//     claims another issuer is refused, not trusted;
//   * a server without a client registration gets one (RFC 7591) as a public
//     client with PKCE and no secret;
//   * the browser comes back to a loopback redirect on an ephemeral port this
//     module listens on; the code is exchanged immediately and stored;
//   * a stored token within 30 s of its expiry is refreshed before a request
//     carries it, and only an http/sse server with no Authorization header of
//     its own is ever given one.
//
// Nothing here imports `vscode`: the browser opener is passed in, so
// `node --test` can drive a whole sign-in against a local fake provider.

import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as http from "node:http";
import * as path from "node:path";

const FETCH_TIMEOUT_MS = 15_000;
const FLOW_TIMEOUT_MS = 300_000;
const EXPIRY_MARGIN_SECONDS = 30;
const MAX_BODY = 200_000;

export interface OAuthMetadata {
  issuer: string;
  authorization_endpoint: string;
  token_endpoint: string;
  registration_endpoint: string;
  scopes_supported: string[];
}

export interface OAuthSession {
  client_id: string;
  client_secret: string;
  redirect_uri: string;
  token_endpoint: string;
  /** RFC 8707 resource indicator, sent on both the authorize and token calls. */
  resource: string;
  scopes: string;
  access_token: string;
  refresh_token: string;
  token_type: string;
  /** 0 means "the provider named no expiry"; never guessed. */
  expires_at: number;
}

export interface OAuthStatus {
  name: string;
  registered: boolean;
  authorized: boolean;
  expires_at: number;
  has_refresh: boolean;
  scopes: string;
}

interface StoreShape {
  sessions: Record<string, OAuthSession>;
}

/** Origin of a server URL, so the well-known paths attach right. */
function originOf(url: string): string {
  const match = /^([a-z][a-z0-9+.-]*:\/\/[^/?#]+)/i.exec(url);
  return match ? match[1] : url;
}

/** Path component without a trailing slash ("" for the origin root). */
function pathOf(url: string): string {
  try {
    return new URL(url).pathname.replace(/\/+$/, "");
  } catch {
    return "";
  }
}

/** Where one URL's well-known documents live, most specific first. */
export function wellKnownCandidates(url: string, wellKnown: string): string[] {
  const origin = originOf(url);
  const path = pathOf(url);
  const bare = url.replace(/\/+$/, "");
  const candidates: string[] = [];
  if (path) candidates.push(`${origin}/${wellKnown}${path}`);
  candidates.push(`${origin}/${wellKnown}`);
  candidates.push(`${bare}/${wellKnown}`);
  return [...new Set(candidates)];
}

function nowSeconds(): number {
  return Date.now() / 1000;
}

async function getJson(
  url: string,
  fetchImpl: typeof fetch,
): Promise<Record<string, unknown>> {
  let response: Response;
  try {
    response = await fetchImpl(url, {
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`Could not reach ${url}: ${(error as Error).message}`);
  }
  const body = await response.text().catch(() => "");
  if (!response.ok) throw new Error(`${url} answered HTTP ${response.status}.`);
  try {
    const parsed = JSON.parse(body) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("not an object");
    }
    return parsed as Record<string, unknown>;
  } catch (error) {
    throw new Error(`${url} is not JSON metadata: ${(error as Error).message}`);
  }
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function metadataFrom(body: Record<string, unknown>, where: string, expectedIssuer: string): OAuthMetadata {
  const issuer = text(body.issuer);
  if (!issuer) throw new Error(`${where} names no issuer.`);
  if (issuer.replace(/\/+$/, "") !== expectedIssuer.replace(/\/+$/, "")) {
    throw new Error(
      `The metadata found at ${where} claims issuer "${issuer}", which does not match where it was found; refusing it.`,
    );
  }
  const metadata: OAuthMetadata = {
    issuer,
    authorization_endpoint: text(body.authorization_endpoint),
    token_endpoint: text(body.token_endpoint),
    registration_endpoint: text(body.registration_endpoint),
    scopes_supported: Array.isArray(body.scopes_supported)
      ? body.scopes_supported.filter((entry): entry is string => typeof entry === "string")
      : [],
  };
  if (!metadata.authorization_endpoint || !metadata.token_endpoint) {
    throw new Error(`${where} does not advertise both an authorization and a token endpoint.`);
  }
  return metadata;
}

async function authorizationMetadata(
  issuer: string,
  fetchImpl: typeof fetch,
): Promise<OAuthMetadata> {
  const wanted = issuer.replace(/\/+$/, "");
  const first = `${wanted}/.well-known/oauth-authorization-server`;
  try {
    return metadataFrom(await getJson(first, fetchImpl), first, wanted);
  } catch (error) {
    // OpenID providers publish the same document under the OIDC spelling.
    const second = `${wanted}/.well-known/openid-configuration`;
    try {
      return metadataFrom(await getJson(second, fetchImpl), second, wanted);
    } catch {
      throw error;
    }
  }
}

/** Discover the resource identifier and the authorization server's metadata. */
export async function discoverOAuth(
  serverUrl: string,
  fetchImpl: typeof fetch = fetch,
): Promise<{ resource: string; metadata: OAuthMetadata }> {
  for (const candidate of wellKnownCandidates(serverUrl, ".well-known/oauth-protected-resource")) {
    let body: Record<string, unknown>;
    try {
      body = await getJson(candidate, fetchImpl);
    } catch {
      continue; // the next spelling is the point of this loop
    }
    const servers = Array.isArray(body.authorization_servers)
      ? body.authorization_servers.filter((entry): entry is string => typeof entry === "string" && Boolean(entry))
      : [];
    const issuer = (servers[0] || originOf(serverUrl)).trim();
    const metadata = await authorizationMetadata(issuer, fetchImpl);
    return { resource: text(body.resource) || serverUrl, metadata };
  }
  for (const candidate of wellKnownCandidates(serverUrl, ".well-known/oauth-authorization-server")) {
    let body: Record<string, unknown>;
    try {
      body = await getJson(candidate, fetchImpl);
    } catch {
      continue;
    }
    const expected = (candidate.split("/.well-known/")[0] || candidate).trim();
    return { resource: serverUrl, metadata: metadataFrom(body, candidate, expected) };
  }
  throw new Error(
    `No OAuth metadata answered at ${serverUrl}: neither a protected-resource document (RFC 9728) nor an authorization-server document (RFC 8414).`,
  );
}

function base64url(buffer: Buffer): string {
  return buffer.toString("base64").replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** A PKCE verifier: 32 random bytes, base64url, which satisfies 43+ chars. */
export function pkceVerifier(): string {
  return base64url(crypto.randomBytes(32));
}

export function pkceChallenge(verifier: string): string {
  return base64url(crypto.createHash("sha256").update(verifier).digest());
}

/** A state value nobody else can guess. */
export function stateValue(): string {
  return base64url(crypto.randomBytes(24));
}

export function buildAuthorizeUrl(input: {
  metadata: OAuthMetadata;
  clientId: string;
  redirectUri: string;
  state: string;
  challenge: string;
  resource: string;
  scopes: string;
}): string {
  const url = new URL(input.metadata.authorization_endpoint);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("client_id", input.clientId);
  url.searchParams.set("redirect_uri", input.redirectUri);
  url.searchParams.set("state", input.state);
  url.searchParams.set("code_challenge", input.challenge);
  url.searchParams.set("code_challenge_method", "S256");
  if (input.resource) url.searchParams.set("resource", input.resource);
  if (input.scopes) url.searchParams.set("scope", input.scopes);
  return url.toString();
}

/** Register a public client (RFC 7591); PKCE is what protects the code. */
export async function registerClient(
  metadata: OAuthMetadata,
  redirectUri: string,
  fetchImpl: typeof fetch = fetch,
): Promise<{ client_id: string; client_secret: string; scopes: string }> {
  if (!metadata.registration_endpoint) {
    throw new Error(
      "This authorization server does not advertise dynamic client registration, and no client is registered yet.",
    );
  }
  let response: Response;
  try {
    response = await fetchImpl(metadata.registration_endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({
        client_name: "SAARTHI-STUDIO MCP",
        redirect_uris: [redirectUri],
        grant_types: ["authorization_code", "refresh_token"],
        response_types: ["code"],
        token_endpoint_auth_method: "none",
      }),
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`Client registration failed: ${(error as Error).message}`);
  }
  const body = await response.text().catch(() => "");
  if (!response.ok) {
    throw new Error(
      `Registration answered HTTP ${response.status} with: ${body.slice(0, 300)}`,
    );
  }
  let answer: Record<string, unknown>;
  try {
    answer = JSON.parse(body) as Record<string, unknown>;
  } catch {
    throw new Error("The registration answer is not JSON.");
  }
  const clientId = text(answer.client_id);
  if (!clientId) throw new Error("The registration answer named no client_id.");
  return {
    client_id: clientId,
    client_secret: text(answer.client_secret),
    scopes: text(answer.scope),
  };
}

function fromTokenAnswer(
  endpoint: string,
  answer: Record<string, unknown>,
  previous: OAuthSession,
): OAuthSession {
  if (text(answer.error)) {
    const detail = text(answer.error_description);
    throw new Error(`The token endpoint refused: ${text(answer.error)}${detail}`);
  }
  const accessToken = text(answer.access_token);
  if (!accessToken) throw new Error("The token answer contained no access_token.");
  const expiresIn = Number(answer.expires_in);
  return {
    ...previous,
    token_endpoint: endpoint,
    access_token: accessToken,
    refresh_token: text(answer.refresh_token) || previous.refresh_token,
    token_type: text(answer.token_type) || previous.token_type || "Bearer",
    expires_at: Number.isFinite(expiresIn) && expiresIn > 0 ? nowSeconds() + expiresIn : 0,
  };
}

async function tokenRequest(
  endpoint: string,
  params: Record<string, string>,
  clientId: string,
  clientSecret: string,
  resource: string,
  fetchImpl: typeof fetch,
): Promise<Record<string, unknown>> {
  const form = new URLSearchParams({ ...params, client_id: clientId });
  if (clientSecret) form.set("client_secret", clientSecret);
  if (resource) form.set("resource", resource);
  let response: Response;
  try {
    response = await fetchImpl(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        Accept: "application/json",
      },
      body: form.toString(),
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch (error) {
    throw new Error(`Talking to the token endpoint failed: ${(error as Error).message}`);
  }
  const body = await response.text().catch(() => "");
  if (!response.ok) {
    throw new Error(`The token endpoint answered HTTP ${response.status} with: ${body.slice(0, 300)}`);
  }
  try {
    return JSON.parse(body) as Record<string, unknown>;
  } catch {
    throw new Error("The token answer is not JSON.");
  }
}

/** Stored OAuth sessions, one per MCP server, in a 0600 file. */
export class McpOAuth {
  private flows = new Map<string, {
    name: string;
    verifier: string;
    resolve: (answer: { code: string; error: string }) => void;
    reject: (error: Error) => void;
  }>();

  constructor(private readonly file: string) {}

  private read(): StoreShape {
    try {
      const parsed = JSON.parse(fs.readFileSync(this.file, "utf8")) as Partial<StoreShape>;
      const sessions: Record<string, OAuthSession> = {};
      const raw = parsed.sessions;
      if (raw && typeof raw === "object") {
        for (const [name, value] of Object.entries(raw as Record<string, unknown>)) {
          if (!value || typeof value !== "object") continue;
          const entry = value as Record<string, unknown>;
          if (typeof entry.client_id !== "string" || typeof entry.token_endpoint !== "string") continue;
          sessions[name] = {
            client_id: entry.client_id,
            client_secret: text(entry.client_secret),
            redirect_uri: text(entry.redirect_uri),
            token_endpoint: entry.token_endpoint,
            resource: text(entry.resource),
            scopes: text(entry.scopes),
            access_token: text(entry.access_token),
            refresh_token: text(entry.refresh_token),
            token_type: text(entry.token_type) || "Bearer",
            expires_at: Number(entry.expires_at) || 0,
          };
        }
      }
      return { sessions };
    } catch {
      return { sessions: {} };
    }
  }

  private write(data: StoreShape): void {
    fs.mkdirSync(path.dirname(this.file), { recursive: true, mode: 0o700 });
    const staging = `${this.file}.tmp-${process.pid}`;
    fs.writeFileSync(staging, `${JSON.stringify(data, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
    fs.renameSync(staging, this.file);
    try {
      fs.chmodSync(this.file, 0o600);
    } catch {
      // Windows has no POSIX mode; the file still lives in the app's data folder.
    }
  }

  get(name: string): OAuthSession | null {
    return this.read().sessions[name] || null;
  }

  status(name: string): OAuthStatus {
    const session = this.get(name);
    return {
      name,
      registered: Boolean(session?.client_id),
      authorized: Boolean(session?.access_token),
      expires_at: session?.expires_at || 0,
      has_refresh: Boolean(session?.refresh_token),
      scopes: session?.scopes || "",
    };
  }

  save(name: string, session: OAuthSession): void {
    const data = this.read();
    data.sessions[name] = session;
    this.write(data);
  }

  forget(name: string): boolean {
    const data = this.read();
    if (!data.sessions[name]) return false;
    delete data.sessions[name];
    this.write(data);
    return true;
  }

  /** Refresh with the stored refresh token and save the renewed session. */
  async refresh(name: string, fetchImpl: typeof fetch = fetch): Promise<OAuthSession> {
    const existing = this.get(name);
    if (!existing) throw new Error(`"${name}" has no saved OAuth session to refresh.`);
    if (!existing.refresh_token) throw new Error(`"${name}" has no refresh token; sign in again.`);
    const answer = await tokenRequest(
      existing.token_endpoint,
      { grant_type: "refresh_token", refresh_token: existing.refresh_token },
      existing.client_id,
      existing.client_secret,
      existing.resource,
      fetchImpl,
    );
    const renewed = fromTokenAnswer(existing.token_endpoint, answer, existing);
    this.save(name, renewed);
    return renewed;
  }

  /**
   * The header a connection should carry, or {} when this app has no say: a
   * server with its own Authorization header keeps it, a stdio server needs
   * none, and no saved session means the server must answer 401 first.
   */
  async header(
    server: { name: string; transport: string; headers: Record<string, string> },
    fetchImpl: typeof fetch = fetch,
  ): Promise<Record<string, string>> {
    if (Object.keys(server.headers || {}).some((key) => key.toLowerCase() === "authorization")) return {};
    if (server.transport !== "http" && server.transport !== "sse") return {};
    let session = this.get(server.name);
    if (!session || !session.access_token) return {};
    const expired = session.expires_at > 0 && session.expires_at - EXPIRY_MARGIN_SECONDS <= nowSeconds();
    if (expired) {
      if (!session.refresh_token) return {};
      try {
        session = await this.refresh(server.name, fetchImpl);
      } catch {
        // A refused refresh leaves the server unauthorized; the call that
        // follows answers 401 rather than sending a token known to be stale.
        return {};
      }
    }
    return { Authorization: `${session.token_type || "Bearer"} ${session.access_token}` };
  }

  /**
   * One sign-in: discover, register if needed, open the browser, wait for the
   * loopback callback, exchange the code and store the session.
   */
  async authorize(input: {
    name: string;
    url: string;
    openBrowser: (url: string) => void;
    fetchImpl?: typeof fetch;
    timeoutMs?: number;
  }): Promise<OAuthStatus> {
    const fetchImpl = input.fetchImpl || fetch;
    const listener = await this.listen();
    try {
      const redirectUri = `http://127.0.0.1:${listener.port}/callback`;
      const { resource, metadata } = await discoverOAuth(input.url, fetchImpl);
      let session = this.get(input.name);
      if (!session || !session.client_id) {
        const registered = await registerClient(metadata, redirectUri, fetchImpl);
        session = {
          client_id: registered.client_id,
          client_secret: registered.client_secret,
          redirect_uri: redirectUri,
          token_endpoint: metadata.token_endpoint,
          resource,
          scopes: registered.scopes,
          access_token: "",
          refresh_token: "",
          token_type: "Bearer",
          expires_at: 0,
        };
        this.save(input.name, session);
      }
      session = {
        ...session,
        redirect_uri: redirectUri,
        token_endpoint: metadata.token_endpoint,
        resource,
      };
      this.save(input.name, session);
      const state = stateValue();
      const verifier = pkceVerifier();
      const authorizeUrl = buildAuthorizeUrl({
        metadata,
        clientId: session.client_id,
        redirectUri,
        state,
        challenge: pkceChallenge(verifier),
        resource,
        scopes: session.scopes,
      });
      // Armed synchronously, then the browser opens: a provider that redirects
      // instantly still finds its state waiting.
      const answer = listener.arm(state, input.name, verifier, input.timeoutMs || FLOW_TIMEOUT_MS);
      input.openBrowser(authorizeUrl);
      const callback = await answer;
      if (callback.error) throw new Error(`The authorization server said: ${callback.error}`);
      if (!callback.code) throw new Error("The browser came back without an authorization code.");
      const stored = this.get(input.name);
      if (!stored) throw new Error("The client registration vanished mid-sign-in; start again.");
      const tokenAnswer = await tokenRequest(
        stored.token_endpoint,
        {
          grant_type: "authorization_code",
          code: callback.code,
          code_verifier: verifier,
          redirect_uri: redirectUri,
        },
        stored.client_id,
        stored.client_secret,
        stored.resource,
        fetchImpl,
      );
      this.save(input.name, fromTokenAnswer(stored.token_endpoint, tokenAnswer, stored));
      return this.status(input.name);
    } finally {
      await listener.close();
    }
  }

  /** A loopback listener for the redirect: ephemeral port, one callback path. */
  private async listen(): Promise<{
    port: number;
    arm: (
      state: string,
      name: string,
      verifier: string,
      timeoutMs: number,
    ) => Promise<{ code: string; error: string }>;
    close: () => Promise<void>;
  }> {
    const server = http.createServer((request, response) => {
      const url = new URL(`http://127.0.0.1${request.url || "/"}`);
      if (request.method !== "GET" || url.pathname !== "/callback") {
        response.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        response.end("Not found");
        return;
      }
      const state = url.searchParams.get("state") || "";
      const pending = this.flows.get(state);
      const finish = (status: number, body: string) => {
        response.writeHead(status, { "Content-Type": "text/html; charset=utf-8", Connection: "close" });
        response.end(body);
      };
      if (!pending) {
        // A state nobody armed is not a sign-in this app started.
        finish(400, "<html><body><p>No sign-in is waiting for that state value.</p></body></html>");
        return;
      }
      this.flows.delete(state);
      finish(
        200,
        "<html><body><p>Authorization received. You can close this window and return to SAARTHI-STUDIO.</p></body></html>",
      );
      pending.resolve({
        code: url.searchParams.get("code") || "",
        error: url.searchParams.get("error_description") || url.searchParams.get("error") || "",
      });
    });
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => resolve());
    });
    const address = server.address();
    const port = typeof address === "object" && address ? address.port : 0;
    return {
      port,
      arm: (state, name, verifier, timeoutMs) =>
        new Promise<{ code: string; error: string }>((settle, fail) => {
          const timer = setTimeout(() => {
            this.flows.delete(state);
            fail(new Error("The browser did not come back within 5 minutes; the sign-in was cancelled."));
          }, timeoutMs);
          this.flows.set(state, {
            name,
            verifier,
            resolve: (value) => {
              clearTimeout(timer);
              settle(value);
            },
            reject: (error) => {
              clearTimeout(timer);
              fail(error);
            },
          });
        }),
      close: async () => {
        for (const [state, pending] of this.flows) {
          this.flows.delete(state);
          pending.reject(new Error("The sign-in flow was dropped; start it again."));
        }
        await new Promise<void>((resolve) => {
          server.closeAllConnections?.();
          server.close(() => resolve());
        });
      },
    };
  }
}

export const OAUTH_LIMITS = { FETCH_TIMEOUT_MS, FLOW_TIMEOUT_MS, MAX_BODY };
