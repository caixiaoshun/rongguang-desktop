"""Generation integration tests with a local mock provider and real media persistence.

Run ONLY against disposable application data: this changes the Grok configuration.
Usage: python scripts/smoke-generation.py http://127.0.0.1:18765
Requires ffmpeg on PATH. No real API key or paid generations are used.
"""
import base64
import json
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

base = sys.argv[1].rstrip('/')
png = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6+0cAAAAASUVORK5CYII=')
captured = []

def call(path, body=None, method=None):
    req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(),
                                 headers={'Content-Type': 'application/json'}, method=method)
    with urllib.request.urlopen(req, timeout=120) as response:
        result = json.load(response)
    assert result['code'] == 0, (path, result.get('msg'))
    return result.get('data')

def wait(kind, task):
    assert isinstance(task, str), 'submit must return a task ID'
    for _ in range(90):
        result = call(f'/api/generation/{kind}/{task}')
        if result['status'] not in (0, 1):
            return result
        time.sleep(0.5)
    raise AssertionError('task timed out')

with tempfile.TemporaryDirectory(prefix='rongguang-media-test-') as folder:
    video_file = Path(folder) / 'clip.mp4'
    subprocess.run(['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'color=c=blue:s=64x64:d=1',
                    '-c:v', 'libx264', '-pix_fmt', 'yuv420p', str(video_file)], check=True)
    video = video_file.read_bytes()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def respond(self, value, mime='application/json'):
            body = json.dumps(value).encode() if mime == 'application/json' else value
            self.send_response(200)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            captured.append((self.path, body))
            if self.path.endswith('/videos/generations'):
                self.respond({'request_id': str(len(captured))})
            else:
                self.respond({'data': [{'b64_json': base64.b64encode(png).decode()}]})
        def do_GET(self):
            if self.path == '/clip.mp4': self.respond(video, 'video/mp4')
            else: self.respond({'status': 'done', 'video': {'url': f'http://127.0.0.1:{mock.server_port}/clip.mp4', 'duration': 1}})

    mock = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=mock.serve_forever, daemon=True)
    thread.start()
    configs = call('/api/ai/api-config/list')
    config = next(c for c in configs if c['platform'] == 'grok')
    call('/api/ai/api-config/update', {'id': config['id'], 'name': 'Disposable mock', 'platform': 'grok',
         'apiUrl': f'http://127.0.0.1:{mock.server_port}', 'apiKey': 'test-key', 'status': 1}, 'PUT')
    models = call('/api/ai/model/list')
    image_model = next(m['id'] for m in models if m['code'] == 'grok-imagine-image')
    video_model = next(m['id'] for m in models if m['code'] == 'grok-imagine-video')
    # Cross the previous 2MB multipart limit with a valid PNG plus trailing padding.
    boundary = 'rongguang-upload-test'
    body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="reference.png"\r\nContent-Type: image/png\r\n\n'.replace('\r\n\n', '\r\n\r\n').encode()
            + png + b'\0' * (3 * 1024 * 1024) + f'\r\n--{boundary}--\r\n'.encode())
    req = urllib.request.Request(base + '/api/storage/upload', data=body, headers={'Content-Type': f'multipart/form-data; boundary={boundary}'})
    with urllib.request.urlopen(req) as response: uploaded = json.load(response)
    assert uploaded['code'] == 0, uploaded
    reference = uploaded['data']
    task = call('/api/generation/image/submit', {'modelId': image_model, 'prompt': 'test', 'refImageUrls': json.dumps([reference]), 'count': 2})
    image_task = wait('image', task)
    assert image_task['status'] == 2 and image_task['successCount'] == 2, image_task
    edits = [body for path, body in captured if path.endswith('/images/edits')]
    assert len(edits) == 2 and edits[0]['images'][0]['url'].startswith('data:image/png;base64,')
    images = call(f"/api/generation/image/{image_task['id']}/items")
    assert len(images) == 2
    assert urllib.request.urlopen(base + images[0]['imageUrl']).read() == png
    task = call('/api/generation/video/submit', {'modelId': video_model, 'prompt': 'test', 'count': 2, 'firstFrameImageUrl': reference})
    video_task = wait('video', task)
    assert video_task['status'] == 2 and video_task['successCount'] == 2, video_task
    videos = call(f"/api/generation/video/{video_task['id']}/items")
    assert len(videos) == 2
    assert urllib.request.urlopen(base + videos[0]['videoUrl']).read() == video
    assert all(item['coverUrl'] for item in videos), 'cover extraction failed'
    project = call('/api/project', {'name': 'Composition smoke test'})
    try:
        board = call(f"/api/storyboard/project/{project['id']}")
        episode = call('/api/storyboard/episode', {'storyboardId': board['id'], 'title': 'Composition'})
        for item in videos:
            call('/api/storyboard/item', {'storyboardId': board['id'], 'storyboardEpisodeId': episode['id'], 'generatedVideoUrl': item['videoUrl']})
        composed = call(f"/api/storyboard/episode/{episode['id']}/compose-video", {}, 'POST')
        output = Path(folder) / 'composed.mp4'
        output.write_bytes(urllib.request.urlopen(base + composed).read())
        duration = subprocess.check_output(['ffprobe', '-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', str(output)])
        assert float(duration) >= 1.9, duration
    finally:
        call(f"/api/project/{project['id']}", method='DELETE')
    for kind, row in [('image', image_task), ('video', video_task)]:
        call(f"/api/generation/{kind}/{row['id']}", method='DELETE')
        history = call(f'/api/generation/{kind}/page')
        assert not any(x['id'] == row['id'] for x in history['list'])
    mock.shutdown()
print('PASS: 3MB upload, image editing, 2 images, 2 videos, local files, covers, composition, deletion')
