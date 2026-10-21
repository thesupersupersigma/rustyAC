# SPDX-License-Identifier: MIT OR Apache-2.0
# rustyAC's server panel (see server/README.md). Node runs the TypeScript as it is (type
# stripping): no build step, no runtime packages.
FROM node:24.11.1-alpine
WORKDIR /panel
COPY package.json ./
COPY src ./src
COPY public ./public
ENV PANEL_DATA=/data PANEL_PACK=/pack PANEL_LISTEN=0.0.0.0
USER node
CMD ["node", "src/main.ts"]
