#!/usr/bin/env bash
# Installs what building the Android app needs, for the current user only:
# JDK 21 and Gradle (mise), the Android SDK and NDK (~/Android/Sdk), cargo-ndk.
# The SDK step asks you to accept Google's SDK licenses.
set -euo pipefail

JDK=java@temurin-21.0.12+8.0.LTS
GRADLE=gradle@8.14.6
SDK="${ANDROID_HOME:-$HOME/Android/Sdk}"
NDK_VERSION=28.2.13676358

mise install "$JDK" "$GRADLE"
export JAVA_HOME="$(mise where "$JDK")"

if [[ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]]; then
  tmp="$(mktemp -d)"
  curl -sSL -o "$tmp/tools.zip" https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip
  mkdir -p "$SDK/cmdline-tools"
  unzip -q "$tmp/tools.zip" -d "$tmp"
  mv "$tmp/cmdline-tools" "$SDK/cmdline-tools/latest"
fi

"$SDK/cmdline-tools/latest/bin/sdkmanager" --licenses
"$SDK/cmdline-tools/latest/bin/sdkmanager" \
  "platforms;android-36" "build-tools;36.0.0" "ndk;$NDK_VERSION" "platform-tools"

command -v cargo-ndk >/dev/null || cargo install cargo-ndk --locked

cat <<EOF

Done. To build:
  export JAVA_HOME=$JAVA_HOME
  export ANDROID_HOME=$SDK
  export ANDROID_NDK_HOME=$SDK/ndk/$NDK_VERSION
  ./scripts/build-android.sh && (cd apps/android && mise exec $GRADLE -- gradle assembleDebug)
EOF
