#!/bin/bash

# AutoTrim Desktop - Setup Verification Script
# Checks if all prerequisites are installed and configured correctly

echo "🔍 AutoTrim Desktop - Setup Verification"
echo "========================================"
echo ""

ERRORS=0
WARNINGS=0

# Check Node.js
echo "📦 Checking Node.js..."
if command -v node &> /dev/null; then
    NODE_VERSION=$(node --version)
    echo "   ✅ Node.js installed: $NODE_VERSION"
    
    # Check if version is >= 18
    MAJOR_VERSION=$(echo $NODE_VERSION | cut -d'.' -f1 | sed 's/v//')
    if [ "$MAJOR_VERSION" -lt 18 ]; then
        echo "   ⚠️  Warning: Node.js version should be 18 or higher"
        WARNINGS=$((WARNINGS + 1))
    fi
else
    echo "   ❌ Node.js not found"
    echo "      Install: https://nodejs.org/"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# Check npm
echo "📦 Checking npm..."
if command -v npm &> /dev/null; then
    NPM_VERSION=$(npm --version)
    echo "   ✅ npm installed: $NPM_VERSION"
else
    echo "   ❌ npm not found"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# Check Rust
echo "🦀 Checking Rust..."
if command -v cargo &> /dev/null; then
    CARGO_VERSION=$(cargo --version)
    echo "   ✅ Rust/Cargo installed: $CARGO_VERSION"
else
    echo "   ❌ Rust not found"
    echo "      Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# Check FFmpeg
echo "🎬 Checking FFmpeg..."
if command -v ffmpeg &> /dev/null; then
    FFMPEG_VERSION=$(ffmpeg -version 2>&1 | head -n1)
    echo "   ✅ FFmpeg installed: $FFMPEG_VERSION"
else
    echo "   ❌ FFmpeg not found"
    echo "      macOS:   brew install ffmpeg"
    echo "      Windows: choco install ffmpeg"
    echo "      Linux:   sudo apt install ffmpeg"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# Check OpenAI API Key
echo "🔑 Checking OpenAI API Key..."
ENV_FILE="/root/.openclaw/workspace/.env"
if [ -f "$ENV_FILE" ]; then
    if grep -q "OPENAI_API_KEY" "$ENV_FILE"; then
        KEY_LENGTH=$(grep "OPENAI_API_KEY" "$ENV_FILE" | cut -d'=' -f2 | tr -d '"' | tr -d "'" | wc -c)
        if [ "$KEY_LENGTH" -gt 10 ]; then
            echo "   ✅ OpenAI API key found in .env"
        else
            echo "   ⚠️  API key found but appears empty"
            WARNINGS=$((WARNINGS + 1))
        fi
    else
        echo "   ⚠️  .env file exists but no OPENAI_API_KEY found"
        WARNINGS=$((WARNINGS + 1))
    fi
else
    echo "   ⚠️  .env file not found at $ENV_FILE"
    echo "      Create it with: echo 'OPENAI_API_KEY=\"your-key\"' > $ENV_FILE"
    WARNINGS=$((WARNINGS + 1))
fi
echo ""

# Check node_modules
echo "📚 Checking dependencies..."
if [ -d "node_modules" ]; then
    echo "   ✅ node_modules found (dependencies installed)"
else
    echo "   ⚠️  node_modules not found"
    echo "      Run: npm install"
    WARNINGS=$((WARNINGS + 1))
fi
echo ""

# Check project structure
echo "📁 Checking project structure..."
REQUIRED_FILES=(
    "package.json"
    "src/App.tsx"
    "src/main.tsx"
    "src-tauri/Cargo.toml"
    "src-tauri/src/main.rs"
    "src-tauri/src/lib.rs"
    "tailwind.config.js"
    "vite.config.ts"
)

MISSING_FILES=0
for file in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$file" ]; then
        echo "   ❌ Missing: $file"
        MISSING_FILES=$((MISSING_FILES + 1))
    fi
done

if [ $MISSING_FILES -eq 0 ]; then
    echo "   ✅ All required files present"
else
    echo "   ❌ $MISSING_FILES required files missing"
    ERRORS=$((ERRORS + 1))
fi
echo ""

# Summary
echo "========================================"
echo "📊 Summary"
echo "========================================"
echo ""

if [ $ERRORS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo "✅ All checks passed! You're ready to run the app."
    echo ""
    echo "Next steps:"
    echo "  1. npm run tauri dev"
    echo "  2. Test with: ./create-test-video.sh"
    echo ""
elif [ $ERRORS -eq 0 ]; then
    echo "⚠️  Setup complete with $WARNINGS warning(s)"
    echo "   You can proceed, but check the warnings above"
    echo ""
    echo "Next steps:"
    echo "  1. npm run tauri dev"
    echo ""
else
    echo "❌ Setup incomplete: $ERRORS error(s), $WARNINGS warning(s)"
    echo "   Please fix the errors above before running the app"
    echo ""
fi

exit $ERRORS
