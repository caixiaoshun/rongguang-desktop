"""One-run credential handoff and encrypted evidence for live desktop tests.

Only public keys and authenticated ciphertext may be published. Private keys,
provider credentials and application databases stay outside artifact paths.
"""
import argparse
import base64
import io
import json
import os
from pathlib import Path
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers.aead import AESGCM


def private_write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(0o600)


def generate_keys(folder):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    key = rsa.generate_private_key(public_exponent=65537, key_size=3072)
    private_write(folder / "private.pem", key.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption()))
    public = key.public_key().public_bytes(
        serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo)
    (folder / "public.pem").write_bytes(public)
    return public


def encrypt(data, public_pem, context):
    public = serialization.load_pem_public_key(public_pem)
    key, nonce = AESGCM.generate_key(bit_length=256), os.urandom(12)
    wrapped = public.encrypt(key, padding.OAEP(
        mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=None))
    ciphertext = AESGCM(key).encrypt(nonce, data, context.encode())
    return {"version": 1, "context": context,
            "wrappedKey": base64.b64encode(wrapped).decode(),
            "nonce": base64.b64encode(nonce).decode(),
            "ciphertext": base64.b64encode(ciphertext).decode()}


def decrypt(envelope, private_pem, context):
    if envelope.get("version") != 1 or envelope.get("context") != context:
        raise ValueError("Envelope is not for this run")
    private = serialization.load_pem_private_key(private_pem, password=None)
    key = private.decrypt(base64.b64decode(envelope["wrappedKey"]), padding.OAEP(
        mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=None))
    return AESGCM(key).decrypt(base64.b64decode(envelope["nonce"]),
                              base64.b64decode(envelope["ciphertext"]), context.encode())


def receive(folder, repository, branch, run_id):
    folder = Path(folder)
    path = urllib.parse.quote(f".github/live-input/{run_id}.json", safe="/")
    url = f"https://api.github.com/repos/{repository}/contents/{path}?ref={urllib.parse.quote(branch, safe='')}"
    deadline = time.monotonic() + 1200
    while time.monotonic() < deadline:
        try:
            req = urllib.request.Request(url, headers={
                "Authorization": "Bearer " + os.environ["GH_TOKEN"],
                "Accept": "application/vnd.github+json"})
            with urllib.request.urlopen(req, timeout=20) as response:
                item = json.load(response)
            envelope = json.loads(base64.b64decode(item["content"]))
            plain = decrypt(envelope, (folder / "private.pem").read_bytes(), str(run_id))
            value = json.loads(plain)
            parsed = urllib.parse.urlsplit(value["base_url"])
            if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
                raise ValueError("Expected a credential-free HTTPS base URL")
            if not value.get("api_key") or not value.get("return_public_key"):
                raise ValueError("Incomplete encrypted input")
            private_write(folder / "credentials.json", plain)
            # GitHub processes this command without retaining its plaintext value.
            print("::add-mask::" + value["api_key"], flush=True)
            print("Encrypted credentials accepted for this run", flush=True)
            return
        except urllib.error.HTTPError as exc:
            if exc.code != 404:
                raise RuntimeError(f"Credential polling failed: HTTP {exc.code}") from None
        time.sleep(10)
    raise TimeoutError("No encrypted input received within 20 minutes")


def probe(credentials, output):
    creds = json.loads(Path(credentials).read_text())
    base = creds["base_url"].rstrip("/")
    url = base + ("/models" if base.endswith("/v1") else "/v1/models")
    result = {"operation": "model-discovery", "ok": False}
    try:
        req = urllib.request.Request(url, headers={"Authorization": "Bearer " + creds["api_key"]})
        with urllib.request.urlopen(req, timeout=40) as response:
            value = json.load(response)
            result.update(ok=True, status=response.status,
                          models=[m.get("id") for m in value.get("data", [])])
    except urllib.error.HTTPError as exc:
        result.update(status=exc.code, error=exc.read(4096).decode(errors="replace"))
    except Exception as exc:
        result["error"] = f"{type(exc).__name__}: {exc}"
    target = Path(output)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(result, ensure_ascii=False).replace(creds["api_key"], "[REDACTED]"))
    print("Model discovery:", "passed" if result["ok"] else "failed", flush=True)
    return result["ok"]


def probe_media(credentials, output):
    """Distinguish a provider-side rejection from desktop request handling."""
    creds = json.loads(Path(credentials).read_text())
    base = creds["base_url"].rstrip("/")
    if not base.endswith("/v1"):
        base += "/v1"
    checks = [
        ("image", "/images/generations", {
            "model": "grok-imagine-image-2.0", "prompt": "A small amber cube on a dark desk, no text",
            "n": 1, "size": "1024x1024",
        }),
        ("video", "/videos/generations", {
            "model": "grok-imagine-video-1.5", "prompt": "A static amber cube on a dark desk, subtle moving light",
            "duration": 6, "aspect_ratio": "16:9",
        }),
    ]
    results = []
    for kind, path, body in checks:
        entry = {"kind": kind, "model": body["model"]}
        try:
            request = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={
                "Authorization": "Bearer " + creds["api_key"], "Content-Type": "application/json",
            })
            with urllib.request.urlopen(request, timeout=210) as response:
                value = json.load(response)
                entry.update(status=response.status, accepted=bool(value.get("data") or value.get("request_id") or value.get("id")))
        except urllib.error.HTTPError as exc:
            entry.update(status=exc.code, error=exc.read(2048).decode(errors="replace"))
        except Exception as exc:
            entry["error"] = f"{type(exc).__name__}: {exc}"
        results.append(entry)
        print(f"Independent {kind} request: HTTP {entry.get('status', 'unavailable')}", flush=True)
    Path(output).write_text(json.dumps(results, ensure_ascii=False).replace(creds["api_key"], "[REDACTED]"), encoding="utf-8")


def seal(credentials, source, output, context):
    creds = json.loads(Path(credentials).read_text())
    root = Path(source)
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            if path.suffix.lower() in {".pem", ".db", ".sqlite", ".sqlite3"} or path.name == "credentials.json":
                raise ValueError("Private file must not be included in evidence")
            raw = path.read_bytes()
            if path.suffix.lower() in {".json", ".txt", ".log", ".md", ".csv"}:
                raw = raw.replace(creds["api_key"].encode(), b"[REDACTED]")
            archive.writestr(str(path.relative_to(root)).replace("\\", "/"), raw)
    envelope = encrypt(stream.getvalue(), creds["return_public_key"].encode(), context)
    Path(output).write_text(json.dumps(envelope))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["bootstrap", "receive", "probe", "probe-media", "seal"])
    parser.add_argument("--private-dir", required=True)
    parser.add_argument("--output")
    parser.add_argument("--source")
    args = parser.parse_args()
    folder = Path(args.private_dir)
    credentials = folder / "credentials.json"
    if args.command == "bootstrap":
        public = generate_keys(folder)
        out = Path(args.output)
        out.mkdir(parents=True, exist_ok=True)
        (out / "bootstrap.json").write_text(json.dumps({
            "run_id": os.environ["GITHUB_RUN_ID"], "head_sha": os.environ["GITHUB_SHA"],
            "public_key": public.decode()}))
    elif args.command == "receive":
        receive(folder, os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_REF_NAME"], os.environ["GITHUB_RUN_ID"])
    elif args.command == "probe":
        if not probe(credentials, args.output):
            raise SystemExit(1)
    elif args.command == "probe-media":
        probe_media(credentials, args.output)
    elif args.command == "seal":
        if credentials.exists():
            seal(credentials, args.source, args.output, os.environ["GITHUB_RUN_ID"])


if __name__ == "__main__":
    main()
