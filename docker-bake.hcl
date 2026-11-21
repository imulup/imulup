# Builds every imulup image variant.
#
#   docker buildx bake                     # all variants, local platform, loaded into docker
#   docker buildx bake nodejs              # one variant
#   VERSION=0.2.0 REGISTRY=ghcr.io/acme docker buildx bake --push \
#     --set '*.platform=linux/amd64,linux/arm64'
#
# Tags produced for VERSION=0.2.0:
#   full:   imulup:0.2.0, imulup:latest
#   <lang>: imulup:0.2.0-<lang>, imulup:<lang>

variable "REGISTRY" {
  default = ""
}

variable "IMAGE" {
  default = "imulup"
}

variable "VERSION" {
  default = "dev"
}

variable "PULUMI_VERSION" {
  default = "3.277.0"
}

variable "LANGUAGES" {
  default = ["nodejs", "python", "go", "dotnet", "java"]
}

function "repo" {
  params = []
  result = REGISTRY == "" ? IMAGE : "${REGISTRY}/${IMAGE}"
}

group "default" {
  targets = ["full", "lang"]
}

target "_common" {
  context    = "."
  dockerfile = "Dockerfile"
  args = {
    PULUMI_VERSION = PULUMI_VERSION
  }
  labels = {
    "org.opencontainers.image.version" = VERSION
  }
}

target "full" {
  inherits = ["_common"]
  args = {
    PULUMI_IMAGE = "pulumi/pulumi"
  }
  tags = ["${repo()}:${VERSION}", "${repo()}:latest"]
}

target "lang" {
  name     = lang
  inherits = ["_common"]
  matrix = {
    lang = LANGUAGES
  }
  args = {
    PULUMI_IMAGE = "pulumi/pulumi-${lang}"
  }
  tags = ["${repo()}:${VERSION}-${lang}", "${repo()}:${lang}"]
}
