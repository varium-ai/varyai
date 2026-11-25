import re

with open("src/config/appConfig.tsx", "r") as f:
    content = f.read()

# Fix type definitions that were broken by adding deepseek-harness, glm
# Pattern: | "deepseek-harness",
#          "glm"
content = re.sub(r'\|\s*"deepseek-harness",\s*\n\s*"glm"', '| "deepseek-harness"\n  | "glm"', content)

# Check and fix any duplicated glm
content = content.replace('"glm",\n  "glm"', '"glm"')
content = content.replace('"glm",\n  "glm"]', '"glm"]')

with open("src/config/appConfig.tsx", "w") as f:
    f.write(content)
