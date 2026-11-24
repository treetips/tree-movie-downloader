#!/bin/sh

echo "#==============================================================="
echo "# Homebrewのセットアップ"
echo "#==============================================================="
brew bundle install
if [ $? -ne 0 ]; then
  echo "Error: Failed to install dependencies via Homebrew."
  exit 1
fi
echo ""

echo "#==============================================================="
echo "# Prekのセットアップ"
echo "#==============================================================="
prek install
echo ""

echo "#==============================================================="
echo "# Skillsのセットアップ"
echo "#==============================================================="
SKILLS=("cnwzhu/gpui-skills" "longbridge/gpui-kit" "actionbook/rust-skills")
for skill in "${SKILLS[@]}"; do
  gh skill install "$skill" --agent=opencode --scope=project --all=true -f
done
mkdir -p .dsh/skills
ln -sf .agents/skills/* .dsh/skills/
echo ""

echo "#==============================================================="
echo "# Rustのセットアップ"
echo "#==============================================================="
# TODO ここにRustのセットアップ手順を追加する
