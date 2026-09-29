-- specs/lean/MembraneSpec.lean
--
-- Umbrella module — imports every arithmetic theorem so `lake build`
-- checks the full carve-out in one pass.
import MembraneSpec.Arithmetic.VTRate
import MembraneSpec.Arithmetic.IsqrtMonotone
import MembraneSpec.Arithmetic.FeeSplit
import MembraneSpec.Arithmetic.InterestAccrual
import MembraneSpec.Arithmetic.TrancheWaterfall
import MembraneSpec.Arithmetic.LiqFeeCap
import MembraneSpec.Arithmetic.DecimalMulRatio
import MembraneSpec.Bridge.RustEncoding
