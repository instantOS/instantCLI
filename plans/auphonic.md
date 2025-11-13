
I wrote this simple auphonic CLI in python, add an `ins video auponic` command
which reproduces it in ideomatic rust, and utilizing the utilities already in
this project. I also in the future want to use auhponic audio processing for the
audio track of the videos which get processed. 

The auphonic API key and preset UUID (after finding that one out for the first
time) should be stored in the central video config file

```
~/.config/instant/video.toml
```

Also, the audio file being processed should be hashed, and a cache of processed
files should be kept, just like with the whisper processing (look at that one
for reference). This is not trivial, think hard. 
That way if we process the same audio again, we don't need to do an API call and
can reuse the version from the cache.

Also do not assume the audio is always


```python
#!/usr/bin/env python3

import argparse
import requests
import time
import os
import json

def create_or_get_preset(api_key, preset_name="Auto Podcast Preset"):
    base_url = 'https://auphonic.com/api'
    headers = {
        'Authorization': f'bearer {args.api_key}',
        'Content-Type': 'application/json'
    }

    # First, try to list presets to check if it exists
    try:
        list_resp = requests.get(f'{base_url}/presets.json', headers=headers)
        list_resp.raise_for_status()
        presets = list_resp.json().get('data', [])
        for p in presets:
            if p.get('preset_name') == preset_name:
                print(f'Found existing preset: {preset_name} (UUID: {p["uuid"]})')
                return p['uuid']
    except Exception as e:
        print(f'Error listing presets: {e}. Proceeding to create new one.')

    # Create new preset
    preset_data = {
        "preset_name": preset_name,
        "algorithms": {
            "filtering": True,
            "leveler": True,
            "normloudness": True,
            "loudnesstarget": -19,
            "denoise": True,
            "denoiseamount": 100,
            "silence_cutter": True,
            "filler_cutter": True,
            "cough_cutter": True
        },
        "output_files": [
            {"format": "mp3", "bitrate": "128", "bitrate_mode": "cbr"}
        ]
    }
    try:
        create_resp = requests.post(f'{base_url}/presets.json', headers=headers, json=preset_data)
        create_resp.raise_for_status()
        new_preset = create_resp.json().get('data')
        uuid = new_preset['uuid']
        print(f'Created new preset: {preset_name} (UUID: {uuid})')
        return uuid
    except Exception as e:
        print(f'Error creating preset: {e}')
        raise

def main():
    parser = argparse.ArgumentParser(description='Process a WAV file with Auphonic and save the result next to the original.')
    parser.add_argument('input_file', help='Path to the input WAV file')
    parser.add_argument('--preset', help='Auphonic Preset UUID (optional; will create auto one if not provided)')
    parser.add_argument('--api_key', required=True, help='Auphonic API key')
    args = parser.parse_args()

    base_url = 'https://auphonic.com/api'
    headers = {'Authorization': f'bearer {args.api_key}'}

    # Get or create preset UUID
    preset_uuid = args.preset
    if not preset_uuid:
        preset_uuid = create_or_get_preset(args.api_key)

    file_name = os.path.basename(args.input_file)
    title = os.path.splitext(file_name)[0]

    data = {
        'preset': preset_uuid,
        'title': title,
        'action': 'start'
    }
    files = {'input_file': open(args.input_file, 'rb')}

    try:
        response = requests.post(f'{base_url}/simple/productions.json', headers=headers, data=data, files=files)
        response.raise_for_status()
        prod = response.json()
        uuid = prod['data']['uuid']
        print(f'Production created with UUID: {uuid}')
    except Exception as e:
        print(f'Error creating production: {e}')
        return

    while True:
        try:
            status_resp = requests.get(f'{base_url}/production/{uuid}/status.json', headers=headers)
            status_resp.raise_for_status()
            status_data = status_resp.json()['data']
            status = status_data['status']
            if status == 3:  # Done
                print('Processing completed.')
                break
            elif status == 2:  # Error
                print('Error during processing.')
                return
            print(f'Current status: {status_data["status_string"]}')
            time.sleep(10)
        except Exception as e:
            print(f'Error checking status: {e}')
            time.sleep(10)

    try:
        details_resp = requests.get(f'{base_url}/production/{uuid}.json', headers=headers)
        details_resp.raise_for_status()
        details = details_resp.json()['data']
        output_dir = os.path.dirname(args.input_file) or '.'
        for out_file in details.get('output_files', []):
            filename = out_file.get('filename')
            if not filename:
                continue
            download_url = f"{out_file['download_url']}?bearer_token={args.api_key}"
            print(f'Downloading {filename}...')
            dl_resp = requests.get(download_url, allow_redirects=True, stream=True)
            dl_resp.raise_for_status()
            output_path = os.path.join(output_dir, filename)
            with open(output_path, 'wb') as f:
                for chunk in dl_resp.iter_content(chunk_size=8192):
                    f.write(chunk)
            print(f'Saved processed file to: {output_path}')
    except Exception as e:
        print(f'Error downloading files: {e}')

if __name__ == '__main__':
    main()

```
