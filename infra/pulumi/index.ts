// Sudden Front on Google Cloud: an Artifact Registry repository for the server image, and one
// Compute Engine VM that runs infra/docker-compose.yml (game server, Prometheus, Grafana, cAdvisor)
// with the image pulled from that repository.
//
// The game keeps every room in one process's memory and holds WebSockets open, so it runs on a
// plain VM rather than a service that scales instances out on its own.
//
//   pulumi config set gcp:project <project>
//   pulumi config set imageTag <tag>          # pushed to <region>-docker.pkg.dev/<project>/sudden-front/server:<tag>
//   pulumi config set adminCidr <your IP>/32  # who may open Grafana and Prometheus
//   pulumi up

import * as gcp from "@pulumi/gcp";
import * as pulumi from "@pulumi/pulumi";
import * as fs from "node:fs";
import * as path from "node:path";

const config = new pulumi.Config();
const gcpConfig = new pulumi.Config("gcp");
const project = gcpConfig.require("project");
const region = gcpConfig.require("region");
const zone = gcpConfig.require("zone");
const machineType = config.get("machineType") ?? "e2-standard-2";
const imageTag = config.get("imageTag") ?? "latest";
const adminCidr = config.get("adminCidr") ?? "0.0.0.0/0";
const grafanaPassword = config.getSecret("grafanaPassword") ?? pulumi.secret("admin");

const name = "sudden-front";
const gamePort = 8800;
const grafanaPort = 3000;
const prometheusPort = 9090;

// ---- image registry
const repository = new gcp.artifactregistry.Repository(name, {
  location: region,
  repositoryId: name,
  format: "DOCKER",
  description: "Sudden Front server images (infra/docker/server.Dockerfile)",
});
const imageRepo = pulumi.interpolate`${region}-docker.pkg.dev/${project}/${repository.repositoryId}`;
const serverImage = pulumi.interpolate`${imageRepo}/server:${imageTag}`;

// ---- the VM's identity: it may pull images, and write logs and metrics
const account = new gcp.serviceaccount.Account(name, {
  accountId: `${name}-vm`,
  displayName: "Sudden Front game VM",
});
const member = pulumi.interpolate`serviceAccount:${account.email}`;
new gcp.artifactregistry.RepositoryIamMember(`${name}-pull`, {
  location: region,
  repository: repository.name,
  role: "roles/artifactregistry.reader",
  member,
});
for (const role of ["roles/logging.logWriter", "roles/monitoring.metricWriter"]) {
  new gcp.projects.IAMMember(`${name}-${role.split("/")[1]}`, { project, role, member });
}

// ---- network
const address = new gcp.compute.Address(name, { region });
const tag = `${name}-game`;
new gcp.compute.Firewall(`${name}-game`, {
  network: "default",
  direction: "INGRESS",
  allows: [{ protocol: "tcp", ports: [String(gamePort)] }],
  sourceRanges: ["0.0.0.0/0"],
  targetTags: [tag],
  description: "Players: the game page and its WebSocket",
});
new gcp.compute.Firewall(`${name}-monitoring`, {
  network: "default",
  direction: "INGRESS",
  allows: [{ protocol: "tcp", ports: [String(grafanaPort), String(prometheusPort)] }],
  sourceRanges: [adminCidr],
  targetTags: [tag],
  description: "Grafana and Prometheus, for the admin network only",
});

// ---- what the VM runs: the same compose project as a local `docker compose up`
const infra = path.resolve(__dirname, "..");
const read = (file: string) => fs.readFileSync(path.join(infra, file), "utf8");
const files: Record<string, string> = {
  "docker-compose.yml": read("docker-compose.yml"),
  "prometheus/prometheus.yml": read("prometheus/prometheus.yml"),
  "grafana/provisioning/datasources/prometheus.yml": read("grafana/provisioning/datasources/prometheus.yml"),
  "grafana/provisioning/dashboards/dashboards.yml": read("grafana/provisioning/dashboards/dashboards.yml"),
  "grafana/dashboards/sudden-front.json": read("grafana/dashboards/sudden-front.json"),
};
// Metadata keys may hold letters, digits, - and _ only.
const key = (file: string) => `sf-${file.replace(/[^A-Za-z0-9]/g, "-")}`;

const startupScript = pulumi.interpolate`#!/bin/bash
# Installs Docker, writes the compose project from the instance metadata, and starts it with the
# server image from Artifact Registry. Runs at every boot; every step may run again.
set -euo pipefail
if ! command -v docker >/dev/null; then
  curl -fsSL https://get.docker.com | sh
fi
meta() {
  curl -fsS -H "Metadata-Flavor: Google" "http://metadata.google.internal/computeMetadata/v1/instance/attributes/$1"
}
dir=/opt/sudden-front/infra
${Object.keys(files)
  .map((file) => `mkdir -p "$dir/${path.posix.dirname(file)}" && meta ${key(file)} > "$dir/${file}"`)
  .join("\n")}
cat > "$dir/.env" <<EOF
SERVER_IMAGE=${serverImage}
GRAFANA_PASSWORD=$(meta sf-grafana-password)
EOF
gcloud auth configure-docker ${region}-docker.pkg.dev --quiet
cd "$dir"
docker compose pull server prometheus grafana cadvisor
docker compose up -d --no-build
`;

const instance = new gcp.compute.Instance(name, {
  zone,
  machineType,
  tags: [tag],
  bootDisk: {
    initializeParams: { image: "debian-cloud/debian-12", size: 20, type: "pd-balanced" },
  },
  networkInterfaces: [{ network: "default", accessConfigs: [{ natIp: address.address }] }],
  serviceAccount: { email: account.email, scopes: ["cloud-platform"] },
  metadataStartupScript: startupScript,
  metadata: {
    ...Object.fromEntries(Object.entries(files).map(([file, text]) => [key(file), text])),
    "sf-grafana-password": grafanaPassword,
  },
  allowStoppingForUpdate: true,
});

export const imageRepository = imageRepo;
export const image = serverImage;
export const ip = address.address;
export const gameUrl = pulumi.interpolate`http://${address.address}:${gamePort}`;
export const grafanaUrl = pulumi.interpolate`http://${address.address}:${grafanaPort}`;
export const prometheusUrl = pulumi.interpolate`http://${address.address}:${prometheusPort}`;
export const instanceName = instance.name;
