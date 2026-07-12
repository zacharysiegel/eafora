# Shader naming conventions

Applies to WGSL shader code and the Rust host code that constructs matching
uniform/storage data (transform matrices, bind group layouts, etc.).

## Transform matrices: name by source space -> destination space

Never name a transform matrix by its graphics-pipeline role alone (`model_matrix`,
`view_matrix`, `projection_matrix`, `mvp`). Those names require already knowing the
classic Model/View/Projection pipeline convention to infer which space the matrix
reads from and which space it produces — someone without that background gets no
information from the name.

Instead, name every transform matrix `<source_space>_to_<destination_space>`:

- `object_to_world` (per-instance placement; the "model" matrix)
- `world_to_view` (the "view"/camera matrix)
- `view_to_clip` (the "projection" matrix)

If matrices are pre-multiplied on the host for performance (avoiding per-vertex
matrix chains in the shader), name the combined matrix by its combined source and
destination space, not by an acronym:

- `object_to_clip` (replaces `mvp`)
- `object_to_view` (replaces a combined model-view matrix)

This applies to struct fields, WGSL `var`/`let` bindings, uniform struct members,
and Rust-side variables/fields that hold the corresponding `glam`/`nalgebra` matrix
values.

## Where this diverges from common graphics-code convention

Direct3D/HLSL and most GLSL tutorials name matrices by destination space only
(`World`, `View`, `Projection`) or combine them into the acronym `MVP`. Do not use
either style here — both require pipeline-convention knowledge the name itself
doesn't carry. This repo trades a few extra characters for a name that's correct
even to a reader with no graphics background.
