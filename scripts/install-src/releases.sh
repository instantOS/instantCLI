fetch_release_json() {
	# Fetch all releases and find first one with our assets
	all_releases=$(curl -fsSL \
		-H "Accept: application/vnd.github+json" \
		-H "User-Agent: instantcli-installer" \
		"$API_URL") || fatal "failed to fetch releases metadata"

	# Try each release until we find one with our asset
	release_json=$(find_working_release "$all_releases") || fatal "no working release found with assets for $TARGET"
}

find_working_release() {
	all_releases="$1"

	# Extract complete top-level release objects. Counting braces avoids splitting
	# on nested asset objects and works with both compact and formatted JSON.
	printf '%s' "$all_releases" | awk -v target="$TARGET" -v use_appimage="$USE_APPIMAGE" '
	function release_has_asset(release,    rest, url_end, url) {
		if (release ~ /"draft"[[:space:]]*:[[:space:]]*true/ ||
		    release ~ /"prerelease"[[:space:]]*:[[:space:]]*true/) {
			return 0
	}

		rest = release
		while (match(rest, /"browser_download_url"[[:space:]]*:[[:space:]]*"/)) {
			rest = substr(rest, RSTART + RLENGTH)
			url_end = index(rest, "\"")
			if (url_end == 0) {
				return 0
			}
			url = substr(rest, 1, url_end - 1)

			if (use_appimage == 1) {
				if (url ~ /\.AppImage$/ && url !~ /\.sha256$/) {
					return 1
				}
			} else if (index(url, target) > 0 && url !~ /\.sha256$/ &&
			           url !~ /\.pkg\.tar\.zst$/ && url !~ /-debug-/) {
				return 1
			}

			rest = substr(rest, url_end + 1)
		}
		return 0
	}

	{
		line = $0 "\n"
		for (i = 1; i <= length(line); i++) {
			char = substr(line, i, 1)

			if (depth > 0) {
				object = object char
			}

			if (in_string) {
				if (escaped) {
					escaped = 0
				} else if (char == "\\") {
					escaped = 1
				} else if (char == "\"") {
					in_string = 0
				}
				continue
			}

			if (char == "\"") {
				in_string = 1
			} else if (char == "{") {
				if (depth == 0) {
					object = "{"
				}
				depth++
			} else if (char == "}") {
				depth--
				if (depth == 0) {
					if (release_has_asset(object)) {
						found = 1
						print object
						exit 0
					}
					object = ""
				}
			}
		}
	}
	END {
		if (!found) {
			exit 1
		}
	}
	'
}

find_asset_urls() {
	if [ "$USE_APPIMAGE" -eq 1 ]; then
		asset_url=$(printf '%s\n' "$release_json" | awk '
	            {
	                rest = $0
	                while (match(rest, /"browser_download_url"[[:space:]]*:[[:space:]]*"/)) {
	                    rest = substr(rest, RSTART + RLENGTH)
	                    url_end = index(rest, "\"")
	                    url = substr(rest, 1, url_end - 1)
	                    if (url ~ /\.AppImage$/ && url !~ /\.sha256$/) {
	                        print url
	                        exit
	                    }
	                    rest = substr(rest, url_end + 1)
	                }
	            }
	        ')
		[ -n "$asset_url" ] || fatal "no AppImage found in release"
	else
		asset_url=$(printf '%s\n' "$release_json" | awk -v target="$TARGET" '
	            {
	                rest = $0
	                while (match(rest, /"browser_download_url"[[:space:]]*:[[:space:]]*"/)) {
	                    rest = substr(rest, RSTART + RLENGTH)
	                    url_end = index(rest, "\"")
	                    url = substr(rest, 1, url_end - 1)
	                    if (index(url, target) && url !~ /\.sha256$/ && url !~ /\.pkg\.tar\.zst$/ && url !~ /-debug-/) {
	                        print url
	                        exit
	                    }
	                    rest = substr(rest, url_end + 1)
	                }
	            }
	        ')
		[ -n "$asset_url" ] || fatal "no prebuilt binary or archive found for $TARGET"
	fi

	sha_url=$(printf '%s\n' "$release_json" | awk -v archive="$asset_url" '
	        {
	            rest = $0
	            target_sha = archive ".sha256"
	            while (match(rest, /"browser_download_url"[[:space:]]*:[[:space:]]*"/)) {
	                rest = substr(rest, RSTART + RLENGTH)
	                url_end = index(rest, "\"")
	                url = substr(rest, 1, url_end - 1)
	                if (url == target_sha) {
	                    print url
	                    exit
	                }
	                rest = substr(rest, url_end + 1)
	            }
	        }
	    ')

	version=$(printf '%s\n' "$release_json" | awk '
	        match($0, /"tag_name"[[:space:]]*:[[:space:]]*"/) {
	            rest = substr($0, RSTART + RLENGTH)
	            tag_end = index(rest, "\"")
	            tag = substr(rest, 1, tag_end - 1)
	            sub(/^v/, "", tag)
	            print tag
            exit
        }
    ')
}

verify_checksum() {
	archive_path=$1

	if [ -z "$sha_url" ]; then
		warn "no checksum published for this asset; skipping verification"
		return 0
	fi

	if ! command -v sha256sum >/dev/null 2>&1; then
		warn "sha256sum not available; skipping checksum verification"
		return 0
	fi

	checksum_file="$TMPDIR/$(basename "$archive_path").sha256"
	curl -fsSL -H "User-Agent: instantcli-installer" "$sha_url" -o "$checksum_file" || {
		warn "failed to download checksum file; skipping verification"
		return 0
	}

	checksum_basename=$(basename "$archive_path")
	if ! grep -q "  $checksum_basename$" "$checksum_file" 2>/dev/null; then
		tmp_checksum_file="$checksum_file.tmp"
		if awk -v name="$checksum_basename" '{print $1 "  " name}' "$checksum_file" >"$tmp_checksum_file" 2>/dev/null; then
			mv "$tmp_checksum_file" "$checksum_file"
		else
			warn "failed to normalize checksum file; skipping verification"
			return 0
		fi
	fi

	(cd "$TMPDIR" && sha256sum -c "$(basename "$checksum_file")") || fatal "checksum verification failed"
}
