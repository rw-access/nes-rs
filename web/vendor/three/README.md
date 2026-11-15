Three.js 0.180.0, MIT license. Only the two ES module builds are vendored.
Source: https://registry.npmjs.org/three/-/three-0.180.0.tgz
Archive SHA256: ad66d724565ee29a2467277fa84daa5ed0211d6b8d446e9ef29f6bae0cd14144

Served locally so the VR demo needs no runtime CDN or third-party requests.

The module build has a narrow WebGLAnimation lifecycle backport from upstream:
https://github.com/mrdoob/three.js/blob/dev/src/renderers/webgl/WebGLAnimation.js
`start` and `stop` guard a null context, and the next frame is requested before
the application callback. This lets a failed first XR initialization clean up
without throwing, and lets stopping inside a callback cancel the next frame.
The core build is unmodified. Both normal and failed XR sessions are covered by
`vr-xr-browser-check.mjs`.

The XR layer-selection condition additionally supports the local manager option
`useProjectionLayers = false`, forcing the standard XRWebGLLayer compatibility
path even when the browser exposes projection-layer APIs. Undefined retains
upstream behavior. This is a runtime compatibility policy, not a claim that
projection layers require a particular optional-feature grant. No other vendor
logic is changed. The demo defaults to classic layers; `?xr=layers` opts into
upstream projection-layer selection for controlled comparisons.
