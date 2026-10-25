import './styles.css'
import './theme'
import { bindDemo } from './demo/play'
import { bindDisclosures } from './disclose'
import { bindFlow } from './flow/animate'

export function bindMotion() {
  bindDemo()
  bindFlow()
  bindDisclosures()
}

bindMotion()
